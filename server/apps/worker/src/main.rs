use axum::{Router, routing::get};
use clap::Parser;
use silva_application::media::MediaService;
use silva_application::publishing::{PublishOutcome, PublishingService};
use silva_infrastructure::{
    media_service, object_storage, persistence, publishing_service, site_builder,
};
use silva_shared::{
    config::Config,
    metrics::{METRICS, WorkerRun},
    shutdown_signal, telemetry,
};
use std::{
    future::IntoFuture,
    net::SocketAddr,
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Args {
    /// 执行一次会话与媒体维护后退出，不启动指标监听。
    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::load()?;
    let telemetry = telemetry::init("silva-worker")?;
    let result = run(config, args.once).await;
    telemetry.shutdown().await;
    result
}

async fn run(config: Config, once: bool) -> anyhow::Result<()> {
    let pool = persistence::connect(&config).await?;
    let result = serve(&pool, &config, once).await;
    pool.close().await;
    result
}

async fn serve(pool: &sqlx::PgPool, config: &Config, once: bool) -> anyhow::Result<()> {
    persistence::ready(pool).await?;
    let storage = object_storage(config)?;
    if storage.is_none() {
        tracing::warn!("object storage is not configured; stale media cleanup is skipped");
    }
    let media = media_service(pool.clone(), storage.clone(), config);
    let publishing = publishing_service(pool.clone(), storage, site_builder(config));
    if !publishing.can_build() {
        tracing::warn!(
            "SILVA_SITE_BUILDER_DIR or object storage is not configured; site publish jobs are not processed"
        );
    }
    METRICS
        .worker_interval
        .set(config.worker_interval.as_secs() as i64);
    if once {
        maintain(pool, &media).await?;
        publish_pending(&publishing).await?;
    } else {
        let bind: SocketAddr = std::env::var("SILVA_WORKER_METRICS_BIND")
            .unwrap_or_else(|_| "127.0.0.1:9091".into())
            .parse()?;
        let listener = tokio::net::TcpListener::bind(bind).await?;
        let metrics_pool = pool.clone();
        let router = Router::new()
            .route(
                "/metrics",
                get(move || {
                    let pool = metrics_pool.clone();
                    async move {
                        persistence::update_pool_metrics(&pool);
                        (
                            [(
                                axum::http::header::CONTENT_TYPE,
                                "text/plain; version=0.0.4; charset=utf-8",
                            )],
                            METRICS.render(),
                        )
                    }
                }),
            )
            .route("/health/live", get(|| async { "ok" }));
        let server = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .into_future();
        tracing::info!(address = %bind, "worker metrics listening");
        let serving = async {
            let result = server.await;
            // 先关闭连接池再取消维护，避免 SQLx 归还连接时等待被锁住的 SQL。
            pool.close().await;
            result
        };
        let maintenance = async {
            tokio::select! {
                () = pool.close_event() => {},
                never = maintain_periodically(pool, &media, config.worker_interval) => never,
                never = publish_periodically(&publishing, config.publish_poll_interval) => never,
            }
        };
        let (result, ()) = tokio::join!(serving, maintenance);
        result?;
    }
    Ok(())
}

async fn maintain_periodically(pool: &sqlx::PgPool, media: &MediaService, interval: Duration) -> ! {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if let Err(error) = maintain(pool, media).await {
            tracing::warn!(%error, "worker maintenance will retry next interval");
        }
    }
}

/// 定期处理站点发布任务；未配置构建时不领取任务。
async fn publish_periodically(publishing: &PublishingService, interval: Duration) -> ! {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if let Err(error) = publish_pending(publishing).await {
            tracing::warn!(%error, "site publishing will retry next interval");
        }
    }
}

/// 连续处理可领取的发布任务，直到没有任务为止。
#[tracing::instrument(name = "worker.publish", skip_all, fields(otel.status_code = tracing::field::Empty))]
async fn publish_pending(publishing: &PublishingService) -> anyhow::Result<()> {
    if !publishing.can_build() {
        return Ok(());
    }
    loop {
        match publishing.run_once().await {
            Ok(None) => return Ok(()),
            // 失败的任务按退避时间重新排队；结束本轮，等下一个轮询周期再检查。
            Ok(Some(PublishOutcome::Failed { .. })) => {
                tracing::Span::current().record("otel.status_code", "ERROR");
                return Ok(());
            }
            Ok(Some(_)) => {}
            Err(error) => {
                tracing::Span::current().record("otel.status_code", "ERROR");
                return Err(error.into());
            }
        }
    }
}

/// 清理过期会话，并清理过期未确认与保留期已满的已拒绝媒体。两项任务互不阻塞，分别记录结果；
/// 任一失败时本轮记为失败，日志和 `silva_worker_task_failures_total` 标明失败的任务。
#[tracing::instrument(name = "worker.maintain", skip_all, fields(trace_id = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn maintain(pool: &sqlx::PgPool, media: &MediaService) -> anyhow::Result<()> {
    let trace_id = telemetry::trace_id(&tracing::Span::current());
    tracing::Span::current().record("trace_id", &trace_id);
    let started = Instant::now();
    let sessions = persistence::maintain_sessions(pool).await;
    let purged = media
        .purge_stale(chrono::Utc::now())
        .await
        .map_err(anyhow::Error::from);
    METRICS.record_worker(
        started.elapsed().as_secs_f64(),
        &WorkerRun {
            deleted_sessions: sessions.as_ref().ok().copied(),
            purged_media: purged.as_ref().ok().map(|report| report.purged),
            failed_media: purged.as_ref().map_or(0, |report| report.failed),
        },
    );
    if let Err(error) = &sessions {
        tracing::error!(task = "sessions", %error, "worker session maintenance failed");
    }
    if let Err(error) = &purged {
        tracing::error!(task = "media", %error, "worker media cleanup failed");
    }
    match (sessions, purged) {
        (Ok(deleted), Ok(report)) if report.failed == 0 => {
            tracing::info!(
                deleted_sessions = deleted,
                purged_media = report.purged,
                "worker maintenance complete"
            );
            Ok(())
        }
        // 单条媒体清理失败已逐条记录日志，这些记录排到后续批次末尾。
        (Ok(_), Ok(report)) => {
            tracing::Span::current().record("otel.status_code", "ERROR");
            Err(anyhow::anyhow!(
                "{} stale media records could not be cleaned up; {} purged",
                report.failed,
                report.purged
            ))
        }
        (Err(error), _) | (_, Err(error)) => {
            tracing::Span::current().record("otel.status_code", "ERROR");
            Err(error)
        }
    }
}

use axum::{Router, routing::get};
use clap::Parser;
use silva_infrastructure::persistence;
use silva_shared::{config::Config, metrics::METRICS, shutdown_signal, telemetry};
use std::{
    future::IntoFuture,
    net::SocketAddr,
    time::{Duration, Instant},
};

#[derive(Parser)]
struct Args {
    /// 执行一次会话维护后退出，不启动指标监听。
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
    METRICS
        .worker_interval
        .set(config.worker_interval.as_secs() as i64);
    if once {
        maintain(pool).await?;
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
                never = maintain_periodically(pool, config.worker_interval) => never,
            }
        };
        let (result, ()) = tokio::join!(serving, maintenance);
        result?;
    }
    Ok(())
}

async fn maintain_periodically(pool: &sqlx::PgPool, interval: Duration) -> ! {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if let Err(error) = maintain(pool).await {
            tracing::warn!(%error, "worker maintenance will retry next interval");
        }
    }
}

#[tracing::instrument(name = "worker.maintain_sessions", skip_all, fields(trace_id = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
async fn maintain(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let trace_id = telemetry::trace_id(&tracing::Span::current());
    tracing::Span::current().record("trace_id", &trace_id);
    let started = Instant::now();
    let result = persistence::maintain_sessions(pool).await;
    METRICS.record_worker(
        result.is_ok(),
        started.elapsed().as_secs_f64(),
        result.as_ref().copied().unwrap_or(0),
    );
    match result {
        Ok(deleted) => {
            tracing::info!(deleted_sessions = deleted, "worker maintenance complete");
            Ok(())
        }
        Err(error) => {
            tracing::Span::current().record("otel.status_code", "ERROR");
            tracing::error!(%error, "worker maintenance failed");
            Err(error)
        }
    }
}

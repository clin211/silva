use crate::AppState;
use axum::{extract::State, http::header, response::IntoResponse};
use silva_shared::metrics::{METRICS, unix_seconds};
use sqlx::PgPool;
use std::time::Duration;

pub(crate) async fn endpoint(State(state): State<AppState>) -> impl IntoResponse {
    silva_infrastructure::persistence::update_pool_metrics(&state.pool);
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    )
}

/// 业务状态异步刷新；抓取指标本身不访问数据库，故障时仍保留进程指标和上次成功时间。
/// 调用方应与主服务共同轮询；丢弃此 future 即取消采样，不产生脱离作用域的任务。
pub async fn sample_metrics(pool: &PgPool) -> ! {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        if let Err(error) = refresh(pool).await {
            tracing::warn!(
                error_kind = if error.is_elapsed() {
                    "timeout"
                } else {
                    "database"
                },
                "business metrics refresh failed"
            );
        }
    }
}

async fn refresh(pool: &PgPool) -> Result<(), RefreshError> {
    // 一次查询读取同一快照；发布数量只计当前目标，年龄追溯尚未激活的最早提交。
    let result = tokio::time::timeout(Duration::from_secs(3), sqlx::query_as::<_, (i64, f64, i64, f64, i64, f64)>(
        "SELECT
          (SELECT count(*) FROM publish_jobs p, site_state s WHERE s.singleton AND p.generation=s.desired_generation AND s.desired_generation IS DISTINCT FROM s.active_generation),
          (SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(p.created_at)),0)::float8 FROM publish_jobs p, site_state s WHERE s.singleton AND p.generation>COALESCE(s.active_generation,0) AND p.generation<=s.desired_generation),
          (SELECT count(*) FROM ai_ingestion_runs WHERE status IN ('queued','running')),
          (SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(COALESCE(started_at, slot_date::timestamp AT TIME ZONE 'UTC'))),0)::float8 FROM ai_ingestion_runs WHERE status IN ('queued','running')),
          (SELECT count(*) FROM outbox_events WHERE processed_at IS NULL),
          (SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(created_at)),0)::float8 FROM outbox_events WHERE processed_at IS NULL)"
    ).fetch_one(pool)).await;
    let (publish, publish_age, ingestion, ingestion_age, outbox, outbox_age) = match result {
        Ok(Ok(row)) => row,
        Ok(Err(_)) => {
            METRICS.business_up.set(0);
            return Err(RefreshError(false));
        }
        Err(_) => {
            METRICS.business_up.set(0);
            return Err(RefreshError(true));
        }
    };
    for (queue, pending, age) in [
        ("publish", publish, publish_age),
        ("ingestion", ingestion, ingestion_age),
        ("outbox", outbox, outbox_age),
    ] {
        METRICS.queue.with_label_values(&[queue]).set(pending);
        METRICS
            .queue_age
            .with_label_values(&[queue])
            .set(if pending > 0 { age.max(0.0) as i64 } else { 0 });
    }
    METRICS
        .publish_delayed
        .set(i64::from(publish > 0 && publish_age > 300.0));
    METRICS.business_last_success.set(unix_seconds());
    METRICS.business_up.set(1);
    Ok(())
}

struct RefreshError(bool);
impl RefreshError {
    fn is_elapsed(&self) -> bool {
        self.0
    }
}

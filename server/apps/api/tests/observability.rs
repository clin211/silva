//! 无需数据库：验证依赖故障时的指标可用性、W3C 传播与标签边界。
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use silva_shared::{config::Config, telemetry};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn metrics_survive_database_failure_and_propagate_trace_context() {
    let telemetry = telemetry::init_local("silva-observability-test").unwrap();
    let url = "postgres://unused:unused@127.0.0.1:1/unused";
    let pool = PgPoolOptions::new().connect_lazy(url).unwrap();
    pool.close().await;
    let config = Config {
        database_url: url.into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        production: false,
        jwt_secret: None,
        db_max_connections: 1,
        access_token_ttl: Duration::from_secs(900),
        refresh_token_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        api_rate_per_minute: 1000,
        login_rate_per_minute: 100,
        login_lockout_threshold: 5,
        login_lockout_duration: Duration::from_secs(900),
    };
    let app = silva_api::build_router(config, pool.clone()).await.unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .header(
                    "traceparent",
                    "00-12345678901234567890123456789012-1234567890123456-01",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers()["x-trace-id"],
        "12345678901234567890123456789012"
    );
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        uuid::Uuid::parse_str(&request_id)
            .unwrap()
            .get_version_num(),
        7
    );
    let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].get("request_id").is_none());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/private-sensitive-path?token=secret")
                .method("UNTRUSTED-METHOD")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_client_error());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("silva_http_request_duration_seconds_bucket"));
    assert!(body.contains("route=\"unmatched\""));
    assert!(body.contains("method=\"OTHER\""));
    assert!(!body.contains("private-sensitive-path"));
    assert!(!body.contains("secret"));
    assert!(!body.contains("UNTRUSTED-METHOD"));
    assert!(body.contains("kind=\"pool_closed\""));
    silva_shared::metrics::METRICS.business_up.set(1);
    silva_shared::metrics::METRICS
        .business_last_success
        .set(123);
    silva_shared::metrics::METRICS
        .queue
        .with_label_values(&["publish"])
        .set(7);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), silva_api::sample_metrics(&pool))
            .await
            .is_err()
    );
    let body = silva_shared::metrics::METRICS.render();
    assert!(body.contains("silva_business_metrics_up 0"));
    assert!(body.contains("silva_business_metrics_last_success_timestamp_seconds 123"));
    assert!(body.contains("silva_queue_pending{queue=\"publish\"} 7"));
    telemetry.shutdown().await;
}

use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

type Events = Arc<Mutex<VecDeque<Value>>>;

fn router() -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/events", get(events))
        .route("/alerts", post(receive))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(Events::default())
}

async fn events(State(events): State<Events>) -> Json<VecDeque<Value>> {
    Json(events.lock().await.clone())
}

async fn receive(State(events): State<Events>, body: Bytes) -> StatusCode {
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    println!("{event}");
    let mut events = events.lock().await;
    if events.len() == 100 {
        events.pop_front();
    }
    events.push_back(event);
    StatusCode::OK
}

pub async fn run(bind: SocketAddr) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!("本地告警接收器监听 {bind}");
    axum::serve(listener, router()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn receiver_keeps_only_the_latest_100_notifications() {
        let app = router();
        for index in 0..101 {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/alerts")
                        .body(Body::from(json!({"index": index}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = app
            .oneshot(Request::get("/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let events: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            events,
            (1..101)
                .map(|index| json!({"index": index}))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn receiver_rejects_invalid_json() {
        let response = router()
            .oneshot(Request::post("/alerts").body(Body::from("{")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn receiver_rejects_bodies_over_one_megabyte() {
        let response = router()
            .oneshot(
                Request::post("/alerts")
                    .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}

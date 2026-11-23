use silva_infrastructure::persistence;
use silva_shared::{config::Config, shutdown_signal, telemetry};
use std::future::IntoFuture;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let telemetry = telemetry::init("silva-api")?;
    let result = run(config).await;
    telemetry.shutdown().await;
    result
}

async fn run(config: Config) -> anyhow::Result<()> {
    let pool = persistence::connect(&config).await?;
    let result = serve(config, &pool).await;
    pool.close().await;
    result
}

async fn serve(config: Config, pool: &sqlx::PgPool) -> anyhow::Result<()> {
    persistence::ready(pool).await?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address = %listener.local_addr()?, "API listening");
    let app = silva_api::build_router(config, pool.clone()).await?;
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .into_future();
    let serving = async {
        let result = server.await;
        // 先标记连接池关闭，再取消查询，避免归还连接时的 ping 等待尚未完成的 SQL。
        pool.close().await;
        result
    };
    let sampling = async {
        tokio::select! {
            () = pool.close_event() => {},
            never = silva_api::sample_metrics(pool) => never,
        }
    };
    let (result, ()) = tokio::join!(serving, sampling);
    Ok(result?)
}

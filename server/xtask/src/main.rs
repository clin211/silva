//! 本地观测栈的告警接收、冒烟校验与故障演练工具。

mod fault;
mod http;
mod receiver;
mod smoke;

use std::net::SocketAddr;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Silva 本地观测工具")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 接收本地 Alertmanager 通知，保留最近 100 次事件。
    AlertReceiver {
        #[arg(long, default_value = "0.0.0.0:9094")]
        bind: SocketAddr,
    },
    /// 验证指标、看板、日志和数据库子链路。
    Smoke,
    /// 短暂停止本地 Worker 和数据库，验证告警后恢复。
    FaultSmoke,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::AlertReceiver { bind } => receiver::run(bind).await,
        Command::Smoke => smoke::run().await,
        Command::FaultSmoke => fault::run().await,
    }
}

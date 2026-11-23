use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;
use tokio::process::Command;

use crate::http::{array, client, endpoint, eventually, json};

struct Compose;

impl Compose {
    async fn run(&self, args: &[&str]) -> Result<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("找不到服务端目录")?;
        let output = Command::new("docker")
            .args([
                "compose",
                "-f",
                "docker-compose.yaml",
                "-f",
                "docker-compose.observability.yaml",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .await
            .context("执行 Docker Compose 失败")?;
        ensure!(
            output.status.success(),
            "docker compose {} 失败：{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    async fn restore(&self) -> Result<()> {
        // 两个恢复命令都必须尝试，避免第一个失败阻止另一个服务恢复。
        let postgres = self.run(&["start", "postgres"]).await;
        let worker = self.run(&["start", "worker"]).await;
        match (postgres, worker) {
            (Ok(_), Ok(_)) => {
                println!("已恢复 PostgreSQL 和 Worker");
                Ok(())
            }
            (Err(postgres), Err(worker)) => {
                bail!("PostgreSQL 恢复失败：{postgres:#}；Worker 恢复失败：{worker:#}")
            }
            (Err(error), _) | (_, Err(error)) => Err(error).context("恢复故障演练服务失败"),
        }
    }
}

struct Interrupt {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl Interrupt {
    fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            interrupt: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    async fn wait(&mut self) -> Result<()> {
        #[cfg(unix)]
        tokio::select! {
            _ = self.interrupt.recv() => {},
            _ = self.terminate.recv() => {},
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await?;
        bail!("故障演练已中断")
    }
}

struct Fault {
    client: Client,
    api: String,
    receiver: String,
    started: DateTime<Utc>,
}

impl Fault {
    async fn notifications(&self, status: &str) -> Result<()> {
        let events = json(self.client.get(format!("{}/events", self.receiver))).await?;
        let events = array(&events)?;
        for (name, job) in [
            ("SilvaTargetDown", "silva-worker"),
            ("SilvaDatabaseDown", "postgres"),
            ("SilvaReadinessFailed", "readiness"),
        ] {
            ensure!(
                has_notification(events, name, status, job, self.started),
                "未收到本次演练的 {name} ({job}) {status} 通知"
            );
        }
        Ok(())
    }

    async fn check_failure(&self) -> Result<()> {
        eventually("数据库离线指标", 40, || async {
            let body = self
                .client
                .get(format!("{}/metrics", self.api))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            ensure!(
                body.lines()
                    .any(|line| line == "silva_business_metrics_up 0"),
                "数据库离线时业务采集状态没有标记为 0"
            );
            Ok(())
        })
        .await?;
        println!("PASS：数据库离线时 /metrics 仍返回 200，业务状态标记失败");
        eventually("告警投递", 180, || self.notifications("firing")).await?;
        println!("PASS：Worker、数据库和就绪探测告警已通过 Alertmanager 投递到本地接收器");
        Ok(())
    }

    async fn check_recovery(&self) -> Result<()> {
        eventually("API 恢复就绪", 180, || async {
            self.client
                .get(format!("{}/health/ready", self.api))
                .send()
                .await?
                .error_for_status()?;
            Ok(())
        })
        .await?;
        eventually("恢复通知", 180, || self.notifications("resolved")).await?;
        println!("PASS：API 恢复就绪，三类告警均已发送恢复通知");
        Ok(())
    }
}

fn has_notification(
    events: &[Value],
    name: &str,
    status: &str,
    job: &str,
    started: DateTime<Utc>,
) -> bool {
    events
        .iter()
        .filter_map(|event| event["alerts"].as_array())
        .flatten()
        .any(|alert| {
            alert["labels"]["alertname"] == name
                && alert["labels"]["job"] == job
                && alert["status"] == status
                && alert["startsAt"]
                    .as_str()
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .is_some_and(|time| time >= started)
        })
}

pub async fn run() -> Result<()> {
    let compose = Compose;
    let running = compose
        .run(&["ps", "--status", "running", "--services"])
        .await?;
    for service in [
        "worker",
        "postgres",
        "api",
        "prometheus",
        "alertmanager",
        "alert-receiver",
    ] {
        ensure!(
            running.split_whitespace().any(|name| name == service),
            "故障演练前 {service} 必须已运行"
        );
    }
    let fault = Fault {
        client: client()?,
        api: endpoint("SILVA_SMOKE_API", "http://127.0.0.1:8088"),
        receiver: endpoint("SILVA_SMOKE_RECEIVER", "http://127.0.0.1:9094"),
        started: Utc::now(),
    };
    let mut interrupt = Interrupt::new()?;
    // 停止操作可能部分成功；无论返回错误还是收到中断，都先尝试恢复再退出。
    let result = async {
        compose.run(&["stop", "worker", "postgres"]).await?;
        println!("已停止本地 Worker 和 PostgreSQL，等待真实规则触发");
        tokio::select! {
            result = fault.check_failure() => result,
            result = interrupt.wait() => result,
        }
    }
    .await;
    let restoration = compose.restore().await;
    match (result, restoration) {
        (Ok(()), Ok(())) => {}
        (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
        (Err(error), Err(restoration)) => bail!("演练失败：{error:#}；恢复失败：{restoration:#}"),
    }
    tokio::select! {
        result = fault.check_recovery() => result,
        result = interrupt.wait() => result,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn notifications_ignore_previous_runs_and_wrong_jobs_or_statuses() {
        let started = DateTime::parse_from_rfc3339("2026-11-21T00:00:00Z")
            .unwrap()
            .to_utc();
        let events = vec![json!({"alerts": [
            {"labels": {"alertname": "SilvaTargetDown", "job": "silva-worker"}, "status": "firing", "startsAt": "2026-11-20T23:59:59Z"},
            {"labels": {"alertname": "SilvaTargetDown", "job": "silva-api"}, "status": "firing", "startsAt": "2026-11-21T00:00:01Z"},
            {"labels": {"alertname": "SilvaTargetDown", "job": "silva-worker"}, "status": "resolved", "startsAt": "2026-11-21T00:00:01Z"}
        ]})];
        assert!(!has_notification(
            &events,
            "SilvaTargetDown",
            "firing",
            "silva-worker",
            started
        ));
    }

    #[test]
    fn notifications_accept_current_run_with_fractional_timestamp() {
        let started = DateTime::parse_from_rfc3339("2026-11-21T00:00:00Z")
            .unwrap()
            .to_utc();
        let events = vec![json!({"alerts": [{
            "labels": {"alertname": "SilvaTargetDown", "job": "silva-worker"},
            "status": "firing", "startsAt": "2026-11-21T08:00:00.123456789+08:00"
        }]})];
        assert!(has_notification(
            &events,
            "SilvaTargetDown",
            "firing",
            "silva-worker",
            started
        ));
    }
}

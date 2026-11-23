use std::{env, future::Future, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use tokio::time::{Instant, sleep, timeout_at};

pub fn client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("创建观测 HTTP 客户端失败")
}

pub fn endpoint(variable: &str, default: &str) -> String {
    env::var(variable)
        .unwrap_or_else(|_| default.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

pub async fn json(request: RequestBuilder) -> Result<Value> {
    Ok(request.send().await?.error_for_status()?.json().await?)
}

pub fn array(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .context("响应字段应为数组")
}

pub fn string(value: &Value) -> Result<&str> {
    value.as_str().context("响应字段应为字符串")
}

pub async fn eventually<F, Fut>(description: &str, seconds: u64, mut check: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let error = match timeout_at(deadline, check()).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => error,
            Err(_) => bail!("{description}等待超时（{seconds} 秒）"),
        };
        if Instant::now() + Duration::from_secs(1) >= deadline {
            return Err(error).with_context(|| format!("{description}等待超时（{seconds} 秒）"));
        }
        sleep(Duration::from_secs(1)).await;
    }
}

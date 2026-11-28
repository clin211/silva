use std::{collections::HashSet, env};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use crate::http::{array, client, endpoint, eventually, json as fetch_json, string};

const DASHBOARDS: [&str; 5] = [
    "silva-overview",
    "silva-api",
    "silva-db",
    "silva-worker",
    "silva-business",
];

struct Smoke {
    client: Client,
    api: String,
    prometheus: String,
    grafana: String,
    password: String,
}

impl Smoke {
    fn grafana_request(&self, path: &str) -> RequestBuilder {
        self.client
            .get(format!("{}{path}", self.grafana))
            .basic_auth("admin", Some(&self.password))
    }

    async fn grafana(&self, path: &str) -> Result<Value> {
        fetch_json(self.grafana_request(path))
            .await
            .with_context(|| format!("Grafana {path} 校验失败"))
    }

    async fn targets(&self) -> Result<()> {
        let data = fetch_json(
            self.client
                .get(format!("{}/api/v1/targets", self.prometheus)),
        )
        .await?;
        let targets = array(&data["data"]["activeTargets"])?;
        for job in [
            "silva-api",
            "silva-worker",
            "otel-collector",
            "postgres",
            "liveness",
            "loki",
            "tempo",
        ] {
            ensure!(
                targets.iter().any(|target| target["labels"]["job"] == job),
                "缺少抓取目标 {job}"
            );
        }
        for target in targets {
            ensure!(
                target["health"] == "up",
                "目标 {} 抓取失败：{}",
                target["labels"]["job"],
                target["lastError"]
            );
        }
        Ok(())
    }

    async fn dashboards(&self) -> Result<Value> {
        let search = self.grafana("/api/search?tag=silva").await?;
        for uid in DASHBOARDS {
            ensure!(
                array(&search)?.iter().any(|item| item["uid"] == uid),
                "缺少看板 {uid}"
            );
        }
        println!("PASS：总览及四张专项 Grafana 看板已配置");
        let overview = self.grafana("/api/dashboards/uid/silva-overview").await?;
        let mut home = self.grafana("/api/dashboards/home").await?;
        if let Some(redirect) = home["redirectUri"].as_str() {
            let uid = redirect.split('/').nth(2).context("首页跳转缺少看板 UID")?;
            home = self.grafana(&format!("/api/dashboards/uid/{uid}")).await?;
        }
        ensure!(
            home["dashboard"]["title"] == overview["dashboard"]["title"],
            "默认首页不是总览"
        );
        ensure!(
            home["dashboard"]["panels"] == overview["dashboard"]["panels"],
            "默认首页面板与总览不同"
        );

        let manager = self
            .grafana("/api/datasources/uid/silva-alertmanager")
            .await?;
        ensure!(
            manager["jsonData"]["implementation"] == "prometheus",
            "Alertmanager 数据源类型错误"
        );
        let status = self
            .grafana("/api/datasources/proxy/uid/silva-alertmanager/api/v2/status")
            .await?;
        ensure!(
            matches!(
                status["cluster"]["status"].as_str(),
                Some("ready" | "disabled")
            ),
            "Alertmanager 集群未就绪"
        );
        array(
            &self
                .grafana("/api/alertmanager/silva-alertmanager/api/v2/silences")
                .await?,
        )?;
        let rules = self
            .grafana("/api/prometheus/silva-prometheus/api/v1/rules")
            .await?;
        let mut has_error_rule = false;
        for group in array(&rules["data"]["groups"])? {
            has_error_rule |= array(&group["rules"])?
                .iter()
                .any(|rule| rule["name"] == "SilvaHighErrorRate");
        }
        ensure!(has_error_rule, "Grafana 中缺少 SilvaHighErrorRate 规则");
        println!("PASS：默认首页、外部告警规则和 Alertmanager 静默接口可用");
        Ok(overview["dashboard"].clone())
    }

    async fn overview_queries(&self, overview: &Value) -> Result<()> {
        for panel in array(&overview["panels"])? {
            let Some(targets) = panel.get("targets") else {
                continue;
            };
            for target in array(targets)? {
                let expr = string(&target["expr"])?.replace("$__rate_interval", "1m");
                let result = fetch_json(
                    self.grafana_request(
                        "/api/datasources/proxy/uid/silva-prometheus/api/v1/query",
                    )
                    .query(&[("query", expr)]),
                )
                .await?;
                ensure!(
                    result["status"] == "success",
                    "总览查询失败：{}",
                    panel["title"]
                );
            }
        }
        println!("PASS：全部总览查询可通过 Grafana 执行");
        Ok(())
    }

    async fn logs(&self, expr: &str, request_id: &str, range: Option<(i64, i64)>) -> Result<()> {
        let mut request = self
            .grafana_request("/api/datasources/proxy/uid/silva-loki/loki/api/v1/query_range")
            .query(&[("query", expr), ("limit", "1000")]);
        if let Some((start_ms, end_ms)) = range {
            request =
                request.query(&[("start", start_ms * 1_000_000), ("end", end_ms * 1_000_000)]);
        }
        let data = fetch_json(request).await?;
        ensure!(
            array(&data["data"]["result"])?
                .iter()
                .any(|stream| stream["stream"]["request_id"] == request_id),
            "Loki 未找到请求 {request_id}"
        );
        Ok(())
    }

    async fn trace(&self, trace_id: &str) -> Result<()> {
        let data = self
            .grafana(&format!(
                "/api/datasources/proxy/uid/silva-tempo/api/v2/traces/{trace_id}"
            ))
            .await?;
        let mut spans = Vec::new();
        for resource in array(&data["trace"]["resourceSpans"])? {
            for scope in array(&resource["scopeSpans"])? {
                spans.extend(array(&scope["spans"])?);
            }
        }
        let root = spans
            .iter()
            .find(|span| span["name"] == "POST /api/v1/admin/sessions")
            .context("缺少登录请求根 span")?;
        let root_id = string(&root["spanId"])?;
        ensure!(
            spans
                .iter()
                .any(|span| span["name"] == "db.find_login" && span["parentSpanId"] == root_id),
            "缺少关联的 db.find_login 子 span"
        );
        Ok(())
    }

    async fn navigation(&self, request_id: &str) -> Result<()> {
        let end_ms = Utc::now().timestamp_millis();
        let start_ms = end_ms - 60_000;
        let mut route_queries = HashSet::new();
        for uid in DASHBOARDS {
            let data = self.grafana(&format!("/api/dashboards/uid/{uid}")).await?;
            route_queries.extend(navigation_queries(&data["dashboard"], start_ms, end_ms)?);
        }
        ensure!(!route_queries.is_empty(), "看板缺少路由日志跳转");
        for expr in route_queries {
            self.logs(&expr, request_id, Some((start_ms, end_ms)))
                .await?;
        }
        println!("PASS：看板导航保留时间，路由日志跳转能查到同一次请求");
        Ok(())
    }
}

fn navigation_queries(dashboard: &Value, start_ms: i64, end_ms: i64) -> Result<HashSet<String>> {
    let mut urls = Vec::new();
    for link in array(&dashboard["links"])? {
        let url = string(&link["url"])?;
        if url.starts_with("/d/") {
            ensure!(link["keepTime"] == true, "看板导航没有保留时间范围");
        }
        urls.push(url);
    }
    for panel in array(&dashboard["panels"])? {
        if let Some(links) = panel.pointer("/fieldConfig/defaults/links") {
            for link in array(links)? {
                urls.push(string(&link["url"])?);
            }
        }
    }
    let mut route_queries = HashSet::new();
    for url in urls.into_iter().filter(|url| url.starts_with("/explore?")) {
        let route_link = url.contains("${__field.labels.route:percentencode}");
        let route: String =
            url::form_urlencoded::byte_serialize(b"/api/v1/admin/sessions").collect();
        let expanded = url
            .replace("${__from}", &start_ms.to_string())
            .replace("${__to}", &end_ms.to_string())
            .replace("${__field.labels.route:percentencode}", &route);
        let parsed = Url::parse(&format!("http://localhost{expanded}"))?;
        let params: Vec<_> = parsed.query_pairs().collect();
        ensure!(
            params
                .iter()
                .any(|(key, value)| key == "schemaVersion" && value == "1"),
            "Explore schemaVersion 错误"
        );
        let panes = params
            .iter()
            .find(|(key, _)| key == "panes")
            .context("Explore 缺少 panes 参数")?;
        let panes: Value = serde_json::from_str(&panes.1)?;
        let pane = &panes["logs"];
        ensure!(pane["datasource"] == "silva-loki", "Explore 日志数据源错误");
        ensure!(
            pane["range"] == json!({"from": start_ms.to_string(), "to": end_ms.to_string()}),
            "Explore 时间范围错误"
        );
        if route_link {
            let expr = string(&pane["queries"][0]["expr"])?;
            ensure!(
                expr.contains("| route=\"/api/v1/admin/sessions\""),
                "Explore 丢失路由过滤"
            );
            route_queries.insert(expr.to_owned());
        }
    }
    Ok(route_queries)
}

pub async fn run() -> Result<()> {
    let smoke = Smoke {
        client: client()?,
        api: endpoint("SILVA_SMOKE_API", "http://127.0.0.1:8089"),
        prometheus: endpoint("SILVA_SMOKE_PROMETHEUS", "http://127.0.0.1:9090"),
        grafana: endpoint("SILVA_SMOKE_GRAFANA", "http://127.0.0.1:3001"),
        password: env::var("GRAFANA_ADMIN_PASSWORD")
            .unwrap_or_else(|_| "silva-observability-local".to_owned()),
    };
    eventually("Prometheus 目标", 60, || smoke.targets()).await?;
    println!("PASS：全部 Prometheus 目标可抓取");
    let overview = smoke.dashboards().await?;
    smoke.overview_queries(&overview).await?;

    let trace_id = Uuid::now_v7().simple().to_string();
    // 使用不存在的账户登录：无需凭据即可触发一次数据库查询，返回 401。
    let response = smoke
        .client
        .post(format!("{}/api/v1/admin/sessions", smoke.api))
        .header("traceparent", format!("00-{trace_id}-1234567890123456-01"))
        .json(&json!({"email": "smoke@example.test", "password": "smoke-password"}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::UNAUTHORIZED,
        "登录探测应返回 401，实际为 {}",
        response.status()
    );
    ensure!(
        response
            .headers()
            .get("x-trace-id")
            .context("响应缺少 x-trace-id")?
            .to_str()?
            == trace_id,
        "traceparent 未被继承"
    );
    let request_id = response
        .headers()
        .get("x-request-id")
        .context("响应缺少 x-request-id")?
        .to_str()?;
    ensure!(
        Uuid::parse_str(request_id)?.get_version() == Some(uuid::Version::SortRand),
        "request_id 不是 UUIDv7"
    );
    let expr = format!("{{service_name=\"silva-api\"}} | trace_id=\"{trace_id}\"");
    eventually("Loki 日志", 60, || smoke.logs(&expr, request_id, None)).await?;
    eventually("Tempo 数据库子链路", 60, || smoke.trace(&trace_id)).await?;
    println!("PASS：W3C traceparent、UUIDv7 request_id、Loki 日志和 Tempo 数据库子链路一致");
    smoke.navigation(request_id).await?;

    eventually("业务状态采集", 60, || async {
        let data = fetch_json(
            smoke
                .client
                .get(format!("{}/api/v1/query", smoke.prometheus))
                .query(&[("query", "silva_business_metrics_up{job=\"silva-api\"}")]),
        )
        .await?;
        ensure!(
            array(&data["data"]["result"])?
                .iter()
                .any(|item| item["value"][1] == "1"),
            "业务采集状态未恢复为 1"
        );
        Ok(())
    })
    .await?;
    println!("PASS：业务状态采集成功");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_links_preserve_time_range_and_encode_route() {
        for source in [
            include_str!("../../observability/grafana/dashboards/overview.json"),
            include_str!("../../observability/grafana/dashboards/api.json"),
            include_str!("../../observability/grafana/dashboards/db.json"),
            include_str!("../../observability/grafana/dashboards/worker.json"),
            include_str!("../../observability/grafana/dashboards/business.json"),
        ] {
            let dashboard: Value = serde_json::from_str(source).unwrap();
            navigation_queries(&dashboard, 1_000, 61_000).unwrap();
        }
    }
}

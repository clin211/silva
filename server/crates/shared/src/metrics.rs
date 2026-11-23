use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
    Registry, TextEncoder,
};
use std::{
    sync::LazyLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::default);

pub fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub struct Metrics {
    registry: Registry,
    pub http_requests: IntCounterVec,
    pub http_duration: HistogramVec,
    pub http_in_flight: IntGauge,
    pub db_acquire: HistogramVec,
    pub db_errors: IntCounterVec,
    pub db_duration: HistogramVec,
    pub db_connections: IntGauge,
    pub db_idle: IntGauge,
    pub db_max: IntGauge,
    pub business_up: IntGauge,
    pub business_last_success: IntGauge,
    pub queue: IntGaugeVec,
    pub queue_age: IntGaugeVec,
    pub publish_delayed: IntGauge,
    pub worker_runs: IntCounterVec,
    pub worker_duration: HistogramVec,
    pub worker_last_success: IntGauge,
    pub worker_interval: IntGauge,
    pub worker_deleted_sessions: IntCounter,
}

impl Default for Metrics {
    fn default() -> Self {
        let registry = Registry::new();
        #[cfg(target_os = "linux")]
        registry
            .register(Box::new(
                prometheus::process_collector::ProcessCollector::for_self(),
            ))
            .expect("进程指标必须有效");
        macro_rules! register {
            ($metric:expr) => {{
                let metric = $metric.expect("静态指标定义必须有效");
                registry
                    .register(Box::new(metric.clone()))
                    .expect("指标名称必须唯一");
                metric
            }};
        }
        let started = register!(IntGauge::new(
            "silva_process_start_time_seconds",
            "进程启动时间，Unix 秒"
        ));
        started.set(unix_seconds());
        Self {
            http_requests: register!(IntCounterVec::new(
                Opts::new("silva_http_requests_total", "已完成的 HTTP 请求"),
                &["method", "route", "status", "traffic"]
            )),
            http_duration: register!(HistogramVec::new(
                HistogramOpts::new(
                    "silva_http_request_duration_seconds",
                    "HTTP 响应头生成耗时，不包含响应体传输"
                )
                .buckets(vec![
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
                ]),
                &["method", "route", "traffic"]
            )),
            http_in_flight: register!(IntGauge::new(
                "silva_http_requests_in_flight",
                "正在处理的 HTTP 请求"
            )),
            db_acquire: register!(HistogramVec::new(
                HistogramOpts::new(
                    "silva_db_acquire_duration_seconds",
                    "连接获取耗时；transaction 包含 BEGIN 往返"
                ),
                &["operation"]
            )),
            db_errors: register!(IntCounterVec::new(
                Opts::new("silva_db_errors_total", "数据库错误，有限分类"),
                &["kind"]
            )),
            db_duration: register!(HistogramVec::new(
                HistogramOpts::new(
                    "silva_db_operation_duration_seconds",
                    "数据库适配器操作耗时，包含连接等待与事务"
                ),
                &["operation"]
            )),
            db_connections: register!(IntGauge::new(
                "silva_db_pool_connections",
                "连接池当前连接数"
            )),
            db_idle: register!(IntGauge::new("silva_db_pool_idle", "连接池空闲连接数")),
            db_max: register!(IntGauge::new(
                "silva_db_pool_max_connections",
                "连接池连接数上限"
            )),
            business_up: register!(IntGauge::new(
                "silva_business_metrics_up",
                "最近一次业务状态采集是否成功"
            )),
            business_last_success: register!(IntGauge::new(
                "silva_business_metrics_last_success_timestamp_seconds",
                "最近成功采集业务状态的时间"
            )),
            queue: register!(IntGaugeVec::new(
                Opts::new(
                    "silva_queue_pending",
                    "尚未完成的业务任务；不代表消费者已实现"
                ),
                &["queue"]
            )),
            queue_age: register!(IntGaugeVec::new(
                Opts::new("silva_queue_oldest_age_seconds", "最老未完成任务的年龄"),
                &["queue"]
            )),
            publish_delayed: register!(IntGauge::new(
                "silva_publish_delayed",
                "当前发布目标未激活且等待超过五分钟"
            )),
            worker_runs: register!(IntCounterVec::new(
                Opts::new("silva_worker_runs_total", "会话维护执行次数"),
                &["outcome"]
            )),
            worker_duration: register!(HistogramVec::new(
                HistogramOpts::new("silva_worker_run_duration_seconds", "会话维护执行耗时"),
                &["outcome"]
            )),
            worker_last_success: register!(IntGauge::new(
                "silva_worker_last_success_timestamp_seconds",
                "最近成功执行会话维护的时间"
            )),
            worker_interval: register!(IntGauge::new(
                "silva_worker_interval_seconds",
                "会话维护执行周期"
            )),
            worker_deleted_sessions: register!(IntCounter::new(
                "silva_worker_deleted_sessions_total",
                "会话维护删除的记录数"
            )),
            registry,
        }
    }
}

impl Metrics {
    pub fn render(&self) -> String {
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut output)
            .expect("内存指标编码必须成功");
        String::from_utf8(output).expect("指标必须是 UTF-8")
    }

    pub fn http_started(&self) -> InFlight<'_> {
        self.http_in_flight.inc();
        InFlight(&self.http_in_flight)
    }

    pub fn record_http(&self, method: &str, route: &str, status: u16, elapsed: f64) {
        // 自定义 HTTP 方法统一归为 OTHER，路由只能来自 MatchedPath 或固定回退值。
        let method = match method {
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "CONNECT"
            | "TRACE" => method,
            _ => "OTHER",
        };
        let traffic = if matches!(
            route,
            "/health/live" | "/health/ready" | "/metrics" | "/openapi.json"
        ) {
            "operations"
        } else {
            "application"
        };
        self.http_requests
            .with_label_values(&[method, route, &status.to_string(), traffic])
            .inc();
        self.http_duration
            .with_label_values(&[method, route, traffic])
            .observe(elapsed);
    }

    pub fn db_operation(&self, operation: &'static str) -> OperationTimer<'_> {
        OperationTimer {
            metrics: self,
            operation,
            started: Instant::now(),
        }
    }

    pub fn record_worker(&self, success: bool, elapsed: f64, deleted: u64) {
        let outcome = if success { "success" } else { "error" };
        self.worker_runs.with_label_values(&[outcome]).inc();
        self.worker_duration
            .with_label_values(&[outcome])
            .observe(elapsed);
        if success {
            self.worker_last_success.set(unix_seconds());
            self.worker_deleted_sessions.inc_by(deleted);
        }
    }
}

pub struct InFlight<'a>(&'a IntGauge);
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.dec();
    }
}

pub struct OperationTimer<'a> {
    metrics: &'a Metrics,
    operation: &'static str,
    started: Instant,
}
impl Drop for OperationTimer<'_> {
    fn drop(&mut self) {
        self.metrics
            .db_duration
            .with_label_values(&[self.operation])
            .observe(self.started.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_separates_operations_and_bounds_method_cardinality() {
        let metrics = Metrics::default();
        metrics.record_http("GET", "/api/v1/admin/users/{id}", 500, 0.2);
        metrics.record_http("GET", "/health/ready", 503, 0.01);
        metrics.record_http("UNTRUSTED-METHOD", "unmatched", 405, 0.01);
        let body = metrics.render();
        assert!(body.contains("silva_http_request_duration_seconds_bucket"));
        assert!(body.contains("traffic=\"application\""));
        assert!(body.contains("traffic=\"operations\""));
        assert!(body.contains("method=\"OTHER\""));
        assert!(!body.contains("UNTRUSTED-METHOD"));
        {
            let _guard = metrics.http_started();
            assert_eq!(metrics.http_in_flight.get(), 1);
        }
        assert_eq!(metrics.http_in_flight.get(), 0);
    }

    #[test]
    fn worker_failure_preserves_last_success() {
        let metrics = Metrics::default();
        metrics.record_worker(true, 0.1, 3);
        let last = metrics.worker_last_success.get();
        metrics.record_worker(false, 0.2, 0);
        assert_eq!(metrics.worker_last_success.get(), last);
        assert_eq!(metrics.worker_runs.with_label_values(&["error"]).get(), 1);
        assert_eq!(metrics.worker_deleted_sessions.get(), 3);
    }
}

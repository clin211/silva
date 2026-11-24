//! API 对外时间格式。
//!
//! 服务端内部与数据库一律使用 UTC；响应中的时间统一换算为北京时间（UTC+08:00），
//! 按 RFC 3339 输出并带显式偏移，如 `2026-11-24T14:45:38+08:00`。小数秒按实际精度输出，
//! 整秒时省略。请求中的时间参数接受任意合法 RFC 3339 偏移。

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde::Serializer;

/// API 输出时间使用的固定偏移：UTC+08:00（中国不实行夏令时）。
pub const API_UTC_OFFSET_SECONDS: i32 = 8 * 3600;

/// API 输出时间使用的时区偏移。
pub fn api_offset() -> FixedOffset {
    FixedOffset::east_opt(API_UTC_OFFSET_SECONDS).expect("UTC+08:00 是合法偏移")
}

/// 把 UTC 时间格式化为带 `+08:00` 偏移的 RFC 3339 字符串。
pub fn format_api_time(time: &DateTime<Utc>) -> String {
    time.with_timezone(&api_offset())
        .to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// 供 `#[serde(serialize_with = "silva_shared::time::serialize")]` 使用。
pub fn serialize<S: Serializer>(time: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format_api_time(time))
}

/// 可空时间字段的序列化；`None` 输出为 `null`。
pub fn serialize_option<S: Serializer>(
    time: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match time {
        Some(time) => serialize(time, serializer),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_in_beijing_time_with_explicit_offset() {
        let time = DateTime::parse_from_rfc3339("2026-11-24T06:45:38Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_api_time(&time), "2026-11-24T14:45:38+08:00");
    }

    #[test]
    fn crosses_date_boundary_and_keeps_fractional_seconds() {
        let time = DateTime::parse_from_rfc3339("2026-11-24T20:30:00.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_api_time(&time), "2026-11-25T04:30:00.123456+08:00");
    }

    #[test]
    fn formatted_value_round_trips_to_the_same_instant() {
        let time = Utc::now();
        let parsed = DateTime::parse_from_rfc3339(&format_api_time(&time)).unwrap();
        assert_eq!(parsed.with_timezone(&Utc), time);
        assert_eq!(parsed.offset().local_minus_utc(), API_UTC_OFFSET_SECONDS);
    }
}

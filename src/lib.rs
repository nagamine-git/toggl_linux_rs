use chrono::{DateTime, Utc};

/// Toggl APIのためのRFC3339形式の日付文字列を生成
pub fn format_datetime_for_toggl(dt: DateTime<Utc>) -> String {
    // ISO 8601形式を使用して、Toggl APIの要求に合わせる
    // Zを使ってUTC（GMT）であることを示す
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
} 
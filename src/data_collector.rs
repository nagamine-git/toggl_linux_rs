use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use yup_oauth2::InstalledFlowAuthenticator;
use url;
use urlencoding;
use user_idle::UserIdle;
use std::time::{Duration, Instant};

use crate::config::{AppConfig, GoogleCalendarConfig};

/// ウィンドウ情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    /// ウィンドウID
    pub id: String,
    
    /// ウィンドウタイトル
    pub title: String,
    
    /// クラス名
    pub class: Option<String>,
    
    /// プロセスID
    pub pid: Option<u32>,
    
    /// 取得時刻
    pub timestamp: DateTime<Utc>,
}

/// カレンダーイベント
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// イベントID
    pub id: String,
    
    /// タイトル
    pub title: String,
    
    /// 開始時刻
    pub start_time: DateTime<Utc>,
    
    /// 終了時刻
    pub end_time: DateTime<Utc>,
    
    /// カレンダーID
    pub calendar_id: String,
    
    /// 説明
    pub description: Option<String>,
}

/// 収集データ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedData {
    /// タイムスタンプ
    pub timestamp: DateTime<Utc>,
    
    /// ウィンドウ情報
    pub window: WindowInfo,
    
    /// 同時に発生しているカレンダーイベント
    pub calendar_events: Vec<CalendarEvent>,

    /// システムがアイドル状態かどうか
    pub is_idle: bool,
}

pub struct DataCollector {
    conn: Connection,
    config: AppConfig,
    idle_threshold: Duration,
    last_active: Instant,
    idle_start: Option<Instant>,
    total_idle_time: Duration,
    block_start: Instant,
}

impl DataCollector {
    pub fn new(config: AppConfig) -> Result<Self> {
        let data_dir = Path::new(&config.general.data_dir);
        let db_path = data_dir.join("activity.db");
        let conn = Connection::open(&db_path).context("Failed to open database")?;

        Ok(Self {
            conn,
            config,
            idle_threshold: Duration::from_secs(300), // 5分のアイドルしきい値
            last_active: Instant::now(),
            idle_start: None,
            total_idle_time: Duration::from_secs(0),
            block_start: Instant::now(),
        })
    }

    fn is_idle(&mut self) -> bool {
        if let Ok(idle_time) = UserIdle::get_time() {
            let is_idle = idle_time.as_milliseconds() as u64 > self.idle_threshold.as_millis() as u64;
            
            // アイドル状態の開始時刻を記録
            if is_idle && self.idle_start.is_none() {
                self.idle_start = Some(Instant::now());
            } else if !is_idle && self.idle_start.is_some() {
                // アイドル状態が終了した場合、合計アイドル時間を更新
                if let Some(start) = self.idle_start.take() {
                    self.total_idle_time += start.elapsed();
                }
            }

            // 15分経過したらブロックをリセット
            if self.block_start.elapsed() >= Duration::from_secs(900) {
                self.block_start = Instant::now();
                self.total_idle_time = Duration::from_secs(0);
                self.idle_start = None;
            }

            is_idle
        } else {
            false
        }
    }

    pub async fn collect(&mut self) -> Result<()> {
        // アイドル状態をチェック
        let is_idle = self.is_idle();

        // 現在のアイドル時間を計算
        let current_idle_time = if let Some(start) = self.idle_start {
            self.total_idle_time + start.elapsed()
        } else {
            self.total_idle_time
        };

        // 15分の半分（7.5分 = 450秒）以上がアイドル状態なら記録しない
        if current_idle_time >= Duration::from_secs(450) {
            debug!("More than half of the 15-minute block is idle ({}s), skipping data collection", 
                   current_idle_time.as_secs());
            return Ok(());
        }

        // アクティブウィンドウの情報を取得
        let window = get_active_window().context("Failed to get active window info")?;
        
        // カレンダーイベントを取得
        let calendar_events = if let Some(calendar_config) = &self.config.google_calendar {
            get_calendar_events(calendar_config)
                .await
                .context("Failed to get calendar events")?
        } else {
            Vec::new()
        };

        let data = CollectedData {
            timestamp: Utc::now(),
            window,
            calendar_events,
            is_idle,
        };

        // データを保存
        self.save_data(&data).context("Failed to save collected data")?;

        Ok(())
    }

    fn save_data(&self, data: &CollectedData) -> Result<()> {
        // ウィンドウデータを保存
        self.conn.execute(
            "INSERT INTO window_data (timestamp, window_id, window_title, window_class, pid)
             VALUES (?, ?, ?, ?, ?)",
            params![
                data.window.timestamp.to_rfc3339(),
                data.window.id,
                data.window.title,
                data.window.class,
                data.window.pid,
            ],
        ).context("Failed to insert window data")?;

        // カレンダーイベントを保存
        for event in &data.calendar_events {
            self.conn.execute(
                "INSERT OR REPLACE INTO calendar_events 
                 (event_id, title, start_time, end_time, calendar_id, description)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    event.id,
                    event.title,
                    event.start_time.to_rfc3339(),
                    event.end_time.to_rfc3339(),
                    event.calendar_id,
                    event.description,
                ],
            ).context("Failed to insert calendar event")?;
        }

        Ok(())
    }
}

/// 保存先を初期化する
pub fn init_storage() -> Result<()> {
    // データディレクトリを作成
    let data_dir = Path::new("./data");
    fs::create_dir_all(data_dir).context("Failed to create data directory")?;
    
    // SQLiteデータベースを初期化
    let db_path = data_dir.join("activity.db");
    let conn = Connection::open(&db_path).context("Failed to open database")?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS window_data (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            window_id TEXT NOT NULL,
            window_title TEXT NOT NULL,
            window_class TEXT,
            pid INTEGER
        )",
        [],
    ).context("Failed to create window_data table")?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS calendar_events (
            id INTEGER PRIMARY KEY,
            event_id TEXT NOT NULL,
            title TEXT NOT NULL,
            start_time TEXT NOT NULL,
            end_time TEXT NOT NULL,
            calendar_id TEXT NOT NULL,
            description TEXT
        )",
        [],
    ).context("Failed to create calendar_events table")?;
    
    conn.execute(
        "CREATE TABLE IF NOT EXISTS data_analysis (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            activity TEXT NOT NULL,
            confidence REAL NOT NULL,
            registered INTEGER NOT NULL DEFAULT 0
        )",
        [],
    ).context("Failed to create data_analysis table")?;
    
    info!("Database initialized at {:?}", db_path);
    Ok(())
}

/// アクティブウィンドウの情報を取得する
pub fn get_active_window() -> Result<WindowInfo> {
    // xdotoolを使用してアクティブウィンドウIDを取得
    let output = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .context("Failed to execute xdotool")?;
    
    if !output.status.success() {
        return Err(anyhow::anyhow!("xdotool command failed"));
    }
    
    let window_id = String::from_utf8(output.stdout)
        .context("Failed to parse window ID")?
        .trim()
        .to_string();
    
    debug!("Active window ID: {}", window_id);
    
    // ウィンドウタイトルを取得
    let title_output = Command::new("xdotool")
        .args(["getwindowname", &window_id])
        .output()
        .context("Failed to get window title")?;
    
    let title = String::from_utf8(title_output.stdout)
        .context("Failed to parse window title")?
        .trim()
        .to_string();
    
    // プロセスIDを取得（オプション）
    let pid = get_window_pid(&window_id).ok();
    
    // ウィンドウクラスを取得（オプション）
    let class = get_window_class(&window_id).ok();
    
    Ok(WindowInfo {
        id: window_id,
        title,
        class,
        pid,
        timestamp: Utc::now(),
    })
}

/// ウィンドウのプロセスIDを取得
fn get_window_pid(window_id: &str) -> Result<u32> {
    let output = Command::new("xdotool")
        .args(["getwindowpid", window_id])
        .output()
        .context("Failed to get window PID")?;
    
    if !output.status.success() {
        return Err(anyhow::anyhow!("xdotool getwindowpid command failed"));
    }
    
    let binding = String::from_utf8(output.stdout)
        .context("Failed to parse PID")?;
    let pid_str = binding.trim();
    
    pid_str.parse::<u32>().context("Failed to parse PID as integer")
}

/// ウィンドウのクラス名を取得
fn get_window_class(window_id: &str) -> Result<String> {
    let output = Command::new("xprop")
        .args(["-id", window_id, "WM_CLASS"])
        .output()
        .context("Failed to get window class")?;
    
    if !output.status.success() {
        return Err(anyhow::anyhow!("xprop command failed"));
    }
    
    let class_output = String::from_utf8(output.stdout)
        .context("Failed to parse window class")?;
    
    // WM_CLASS(STRING) = "classname", "classname"
    let parts: Vec<&str> = class_output.split('=').collect();
    if parts.len() < 2 {
        return Err(anyhow::anyhow!("Unexpected xprop output format"));
    }
    
    let class_part = parts[1].trim();
    // 引用符を削除
    let class = class_part.trim_matches(|c| c == '"' || c == '\'' || c == ' ');
    
    Ok(class.to_string())
}

// Google Calendar APIのレスポンス構造体
#[derive(Debug, Deserialize)]
struct EventTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

/// カレンダーイベントを取得
pub async fn get_calendar_events(config: &GoogleCalendarConfig) -> Result<Vec<CalendarEvent>> {
    debug!("Getting calendar events from Google Calendar API");
    
    // 認証情報のデバッグ出力
    debug!("Calendar IDs: {}", config.calendar_ids);
    
    // 現在時刻を取得
    let now = Utc::now();
    
    // 開始時刻（1時間前）と終了時刻（24時間後）を設定
    let time_min = now - chrono::Duration::hours(1);
    let time_max = now + chrono::Duration::hours(24);
    
    debug!("Time range: {} to {}", time_min.to_rfc3339(), time_max.to_rfc3339());
    
    // OAuth2認証情報を構築
    let client_id = config.client_id.clone();
    let client_secret = config.client_secret.clone();
    let refresh_token = config.refresh_token.clone();
    
    if refresh_token.is_empty() {
        warn!("Refresh token is empty. Browser authentication will be required");
    } else {
        debug!("Using refresh token from config file");
    }
    
    // OAuth2認証用クライアントを初期化
    let secret = yup_oauth2::ApplicationSecret {
        client_id,
        client_secret,
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        redirect_uris: vec!["http://localhost".to_string()],
        project_id: None,
        client_email: None,
        auth_provider_x509_cert_url: None,
        client_x509_cert_url: None,
    };
    
    // アクセストークンを取得
    let scopes = &["https://www.googleapis.com/auth/calendar.readonly"];
    
    // リフレッシュトークンがある場合は、それを使用
    let token = if !refresh_token.is_empty() {
        debug!("Attempting to get access token using refresh token");
        
        // リフレッシュトークンを使ってアクセストークンを取得
        let token_url = "https://oauth2.googleapis.com/token";
        debug!("Sending token refresh request to {}", token_url);
        
        let form_data = [
            ("client_id", secret.client_id.as_str()),
            ("client_secret", secret.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        
        debug!("Form data: client_id={}, refresh_token={}...", 
               &secret.client_id, 
               &refresh_token.chars().take(10).collect::<String>());
        
        let response = reqwest::Client::new()
            .post(token_url)
            .form(&form_data)
            .send()
            .await;
        
        match response {
            Ok(res) => {
                let status = res.status();
                if status.is_success() {
                    debug!("Token refresh request successful: {}", status);
                    
                    let json_response = res.json::<serde_json::Value>().await;
                    match json_response {
                        Ok(access_token) => {
                            // JSONレスポンスをデバッグ出力（トークン自体は隠す）
                            debug!("Token response keys: {:?}", 
                                   access_token.as_object()
                                       .map(|obj| obj.keys().collect::<Vec<_>>())
                                       .unwrap_or_default());
                            
                            if let Some(token_str) = access_token.get("access_token").and_then(|t| t.as_str()) {
                                debug!("Successfully obtained access token");
                                token_str.to_string()
                            } else {
                                error!("No access_token in response: {:?}", access_token);
                                return Err(anyhow::anyhow!("Failed to get access token from refresh token"));
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse token response: {}", e);
                            return Err(anyhow::anyhow!("Failed to parse token response: {}", e));
                        }
                    }
                } else {
                    let error_text = match res.text().await {
                        Ok(text) => text,
                        Err(_) => "Failed to read error response".to_string()
                    };
                    error!("Token refresh request failed: {} - {}", status, error_text);
                    return Err(anyhow::anyhow!("Token refresh request failed with status: {}", status));
                }
            }
            Err(e) => {
                error!("Failed to send token refresh request: {}", e);
                return Err(anyhow::anyhow!("Failed to send token refresh request: {}", e));
            }
        }
    } else {
        // リフレッシュトークンがない場合は、通常のブラウザフロー認証
        debug!("No refresh token available, using browser authentication flow");
        let auth = InstalledFlowAuthenticator::builder(secret, yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect)
            .build()
            .await
            .context("Failed to create authenticator")?;
        
        let token_result = auth.token(scopes).await
            .context("Failed to obtain access token")?;
        
        token_result.token().unwrap_or_default().to_string()
    };
    
    debug!("Access token obtained, length: {}", token.len());
    
    // HTTP クライアントを初期化
    let client = reqwest::Client::new();
    
    // カレンダーIDのリストを取得（カンマ区切り文字列から）
    let calendar_ids_str = config.calendar_ids.trim();
    let calendar_ids: Vec<&str> = if calendar_ids_str.is_empty() {
        // カレンダーIDが未設定の場合、主カレンダー（"primary"）を使用
        info!("No calendar IDs specified, using primary calendar");
        vec!["primary"]
    } else {
        calendar_ids_str.split(',').collect()
    };
    
    debug!("Fetching events from {} calendars", calendar_ids.len());
    
    let mut all_events = Vec::new();
    
    // 各カレンダーからイベントを取得
    for calendar_id in calendar_ids {
        let calendar_id = calendar_id.trim();
        if calendar_id.is_empty() {
            warn!("Empty calendar ID found, skipping");
            continue;
        }
        
        debug!("Fetching events from calendar: {}", calendar_id);
        
        // Google Calendar API URLを構築
        let endpoint = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events",
            urlencoding::encode(calendar_id)
        );
        
        let mut url = url::Url::parse(&endpoint)?;
        url.query_pairs_mut()
            .append_pair("timeMin", &time_min.to_rfc3339())
            .append_pair("timeMax", &time_max.to_rfc3339())
            .append_pair("singleEvents", "true")
            .append_pair("orderBy", "startTime")
            .append_pair("maxResults", "100");
        
        debug!("Calendar API URL: {}", url);
        
        // APIリクエストを実行
        let response = client.get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;
        
        match response {
            Ok(res) => {
                let status = res.status();
                if status.is_success() {
                    debug!("Calendar API request successful: {}", status);
                    
                    // レスポンスをテキストとして保存
                    let response_text = res.text().await
                        .context("Failed to get response text")?;
                    
                    debug!("Response length: {} bytes", response_text.len());
                    
                    // 最初の1000文字だけデバッグ出力
                    if response_text.len() > 1000 {
                        debug!("Response preview: {}...", &response_text[..1000]);
                    } else {
                        debug!("Full response: {}", response_text);
                    }
                    
                    // テキストからJSONにパース
                    let parse_result: Result<serde_json::Value, _> = serde_json::from_str(&response_text);
                    
                    match parse_result {
                        Ok(json_response) => {
                            // イベント数を確認
                            if let Some(items) = json_response.get("items").and_then(|i| i.as_array()) {
                                debug!("Retrieved {} events from calendar {}", items.len(), calendar_id);
                                
                                for event in items {
                                    debug!("Processing event: {}", 
                                           event.get("summary")
                                               .and_then(|s| s.as_str())
                                               .unwrap_or("Unknown"));
                                    
                                    let id = event.get("id").and_then(|v| v.as_str());
                                    let summary = event.get("summary").and_then(|v| v.as_str());
                                    
                                    if let (Some(id), Some(summary)) = (id, summary) {
                                        let start_obj = event.get("start");
                                        let end_obj = event.get("end");
                                        
                                        // 開始時刻と終了時刻のデバッグ出力
                                        debug!("Event time data - start: {:?}, end: {:?}", start_obj, end_obj);
                                        
                                        // 開始時刻と終了時刻をパース
                                        match (parse_event_time_from_json(start_obj), parse_event_time_from_json(end_obj)) {
                                            (Ok(start_time), Ok(end_time)) => {
                                                debug!("Successfully parsed event times: {} to {}", 
                                                       start_time.to_rfc3339(), 
                                                       end_time.to_rfc3339());
                                                
                                                // カレンダーイベントを作成
                                                let calendar_event = CalendarEvent {
                                                    id: id.to_string(),
                                                    title: summary.to_string(),
                                                    start_time,
                                                    end_time,
                                                    calendar_id: calendar_id.to_string(),
                                                    description: event.get("description")
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| s.to_string()),
                                                };
                                                
                                                all_events.push(calendar_event);
                                            }
                                            (Err(e1), _) => {
                                                error!("Failed to parse event start time: {}", e1);
                                            }
                                            (_, Err(e2)) => {
                                                error!("Failed to parse event end time: {}", e2);
                                            }
                                        }
                                    } else {
                                        warn!("Event missing id or summary: {:?}", event);
                                    }
                                }
                            } else {
                                warn!("No 'items' field in response or not an array");
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse response as JSON: {}", e);
                        }
                    }
                } else {
                    let error_text = match res.text().await {
                        Ok(text) => text,
                        Err(_) => "Failed to read error response".to_string()
                    };
                    error!("Calendar API request failed: {} - {}", status, error_text);
                }
            }
            Err(e) => {
                error!("Failed to send request to Google Calendar API: {}", e);
            }
        }
    }
    
    if all_events.is_empty() {
        warn!("No calendar events were retrieved");
    } else {
        debug!("Retrieved a total of {} calendar events", all_events.len());
        
        // 取得したイベントの内容をデバッグ出力
        for (i, event) in all_events.iter().enumerate() {
            debug!("Event {}: '{}' @ {} - {}", 
                   i+1, 
                   event.title, 
                   event.start_time.to_rfc3339(), 
                   event.end_time.to_rfc3339());
        }
    }
    
    Ok(all_events)
}

/// JSONからイベント時間を解析
fn parse_event_time_from_json(time_obj: Option<&serde_json::Value>) -> Result<DateTime<Utc>> {
    if let Some(time_obj) = time_obj {
        // dateTimeフィールドを確認
        if let Some(date_time) = time_obj.get("dateTime").and_then(|dt| dt.as_str()) {
            debug!("Parsing dateTime: {}", date_time);
            // RFC3339形式の日時文字列をパース
            let dt = DateTime::parse_from_rfc3339(date_time)
                .context("Failed to parse event datetime")?
                .with_timezone(&Utc);
            Ok(dt)
        } 
        // dateフィールドを確認（終日イベント）
        else if let Some(date) = time_obj.get("date").and_then(|d| d.as_str()) {
            debug!("Parsing date: {}", date);
            // 終日イベントの場合は日付のみが設定されている
            // この場合、時間は00:00:00として処理
            let naive_date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .context("Failed to parse event date")?;
            let naive_datetime = naive_date.and_hms_opt(0, 0, 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid time"))?;
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive_datetime, Utc);
            Ok(dt)
        } else {
            Err(anyhow::anyhow!("Event time object has neither dateTime nor date: {:?}", time_obj))
        }
    } else {
        Err(anyhow::anyhow!("Event has no timing information"))
    }
}

/// イベント時間をUTC DateTimeに変換
fn parse_event_time(event_time: &Option<EventTime>) -> Result<DateTime<Utc>> {
    if let Some(event_time) = event_time {
        if let Some(date_time) = &event_time.date_time {
            // RFC3339形式の日時文字列をパース
            let dt = DateTime::parse_from_rfc3339(date_time)
                .context("Failed to parse event datetime")?
                .with_timezone(&Utc);
            Ok(dt)
        } else if let Some(date) = &event_time.date {
            // 終日イベントの場合は日付のみが設定されている
            // この場合、時間は00:00:00として処理
            let naive_date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .context("Failed to parse event date")?;
            let naive_datetime = naive_date.and_hms_opt(0, 0, 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid time"))?;
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive_datetime, Utc);
            Ok(dt)
        } else {
            Err(anyhow::anyhow!("Event has neither date_time nor date"))
        }
    } else {
        Err(anyhow::anyhow!("Event has no timing information"))
    }
}

/// 最近のデータを取得
pub fn get_recent_data() -> Result<Vec<CollectedData>> {
    let db_path = Path::new("./data").join("activity.db");
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    
    let conn = Connection::open(&db_path)
        .context("Failed to open database")?;
    
    // 直近15分のウィンドウデータを取得
    let cutoff_time = (Utc::now() - chrono::Duration::minutes(15))
        .to_rfc3339();
    
    let mut stmt = conn.prepare(
        "SELECT timestamp, window_id, window_title, window_class, pid 
         FROM window_data 
         WHERE timestamp > ?1
         ORDER BY timestamp DESC"
    ).context("Failed to prepare statement")?;
    
    let window_rows = stmt.query_map(params![cutoff_time], |row| {
        let timestamp: String = row.get(0)?;
        let timestamp = DateTime::parse_from_rfc3339(&timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        
        Ok(WindowInfo {
            id: row.get(1)?,
            title: row.get(2)?,
            class: row.get(3)?,
            pid: row.get(4)?,
            timestamp,
        })
    }).context("Failed to query window data")?;
    
    // ウィンドウ情報をまとめる
    let mut windows = Vec::new();
    for window_result in window_rows {
        match window_result {
            Ok(window) => windows.push(window),
            Err(e) => error!("Error loading window data: {}", e),
        }
    }
    
    // カレンダーイベントを取得
    // 同じ時間枠のカレンダーイベントを検索
    let mut calendar_stmt = conn.prepare(
        "SELECT event_id, title, start_time, end_time, calendar_id, description
         FROM calendar_events
         WHERE start_time <= ?1 AND end_time >= ?1"
    ).context("Failed to prepare calendar statement")?;
    
    // 各時点でのカレンダーイベントのマップを作成
    let mut calendar_events_map: HashMap<DateTime<Utc>, Vec<CalendarEvent>> = HashMap::new();
    
    for window in &windows {
        let timestamp_str = window.timestamp.to_rfc3339();
        let event_rows = calendar_stmt.query_map(params![timestamp_str], |row| {
            let start_time: String = row.get(2)?;
            let end_time: String = row.get(3)?;
            
            let start_time = DateTime::parse_from_rfc3339(&start_time)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            
            let end_time = DateTime::parse_from_rfc3339(&end_time)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            
            Ok(CalendarEvent {
                id: row.get(0)?,
                title: row.get(1)?,
                start_time,
                end_time,
                calendar_id: row.get(4)?,
                description: row.get(5)?,
            })
        }).context("Failed to query calendar events")?;
        
        let mut events = Vec::new();
        for event_result in event_rows {
            match event_result {
                Ok(event) => events.push(event),
                Err(e) => error!("Error loading calendar event: {}", e),
            }
        }
        
        calendar_events_map.insert(window.timestamp, events);
    }
    
    // CollectedDataオブジェクトを作成
    let collected_data = windows.into_iter().map(|window| {
        let events = calendar_events_map.get(&window.timestamp)
            .cloned()
            .unwrap_or_default();
        
        CollectedData {
            timestamp: window.timestamp,
            window: window.clone(),
            calendar_events: events,
            is_idle: false, // 過去のデータは非アイドル状態として扱う
        }
    }).collect();
    
    Ok(collected_data)
}

/// データを収集する
pub async fn collect_data(config: &AppConfig) -> Result<()> {
    // アクティブウィンドウ情報を取得
    let window_info = get_active_window()
        .context("Failed to get active window information")?;
    
    debug!("Collected window info: {}", window_info.title);
    
    // データベースに保存
    let db_path = Path::new(&config.general.data_dir).join("activity.db");
    let conn = Connection::open(&db_path)
        .context("Failed to open database")?;
    
    conn.execute(
        "INSERT INTO window_data (timestamp, window_id, window_title, window_class, pid)
         VALUES (?, ?, ?, ?, ?)",
        params![
            window_info.timestamp.to_rfc3339(),
            window_info.id,
            window_info.title,
            window_info.class,
            window_info.pid,
        ],
    ).context("Failed to insert window data")?;
    
    // カレンダー情報の収集（設定がある場合）
    if let Some(calendar_config) = &config.google_calendar {
        match get_calendar_events(calendar_config).await {
            Ok(events) => {
                debug!("Retrieved {} calendar events", events.len());
                
                // カレンダーイベントをデータベースに保存
                for event in events {
                    conn.execute(
                        "INSERT OR REPLACE INTO calendar_events 
                         (event_id, title, start_time, end_time, calendar_id, description)
                         VALUES (?, ?, ?, ?, ?, ?)",
                        params![
                            event.id,
                            event.title,
                            event.start_time.to_rfc3339(),
                            event.end_time.to_rfc3339(),
                            event.calendar_id,
                            event.description,
                        ],
                    ).context("Failed to insert calendar event")?;
                }
            }
            Err(e) => {
                error!("Failed to get calendar events: {}", e);
            }
        }
    }
    
    Ok(())
}

pub fn get_active_window_pid() -> Result<u32> {
    let output = Command::new("xprop")
        .args(["-root", "_NET_ACTIVE_WINDOW"])
        .output()
        .context("Failed to execute xprop")?;

    let window_id_str = String::from_utf8(output.stdout)
        .context("Failed to parse xprop output")?
        .trim()
        .split(' ')
        .last()
        .context("Failed to extract window ID")?
        .to_string();

    let output = Command::new("xprop")
        .args(["-id", &window_id_str, "_NET_WM_PID"])
        .output()
        .context("Failed to get PID for window")?;

    let pid_output = String::from_utf8(output.stdout)
        .context("Failed to parse PID")?;
    let pid_str = pid_output.trim();

    pid_str.parse::<u32>().context("Failed to parse PID as integer")
}

// Helper function to mask tokens in logs
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        return "[TOKEN_TOO_SHORT_TO_MASK]".to_string();
    }
    
    let visible_prefix = &token[0..4];
    let visible_suffix = &token[token.len() - 4..];
    format!("{}...{}", visible_prefix, visible_suffix)
} 
use anyhow::{anyhow, Result, bail, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use log::{debug, error};
use reqwest;
use serde::{Deserialize, Serialize};
use urlencoding;

/// Togglのワークスペース情報
#[derive(Debug, Deserialize)]
pub struct Workspace {
    pub id: u64,
    pub name: String,
    pub organization_id: u64,
}

/// Togglのプロジェクト情報
#[derive(Debug, Deserialize)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub wid: u64,
    pub cid: Option<u64>,
}

/// Togglのタイムエントリ
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeEntry {
    /// 説明
    pub description: String,
    
    /// ワークスペースID
    #[serde(rename = "wid")]
    pub wid: u64,
    
    /// プロジェクトID（オプション）
    #[serde(rename = "pid", skip_serializing_if = "Option::is_none")]
    pub pid: Option<u64>,
    
    /// 開始時刻
    pub start: String,
    
    /// 終了時刻（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    
    /// 期間（秒単位）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
    
    /// タグ（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    
    /// 作成方法
    #[serde(rename = "created_with", skip_serializing_if = "Option::is_none")]
    pub created_with: Option<String>,
    
    /// メタデータ（オプション）
    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// TimeEntryの作成リクエスト
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateTimeEntryRequest {
    #[serde(rename = "time_entry")]
    pub time_entry: TimeEntry,
}

/// TimeEntryの作成レスポンス
#[derive(Debug, Deserialize)]
struct CreateTimeEntryResponse {
    data: TimeEntryData,
}

/// TimeEntryのデータ
#[derive(Debug, Deserialize)]
struct TimeEntryData {
    id: u64,
}

/// TogglのAPIクライアント
#[derive(Debug, Clone)]
pub struct TogglClient {
    api_token: String,
    client: reqwest::Client,
}

impl TogglClient {
    /// 新しいTogglクライアントを作成
    pub fn new(api_token: String) -> Self {
        let client = reqwest::Client::new();
        Self { api_token, client }
    }
    
    /// 認証ヘッダーを生成
    pub fn auth_header(&self) -> String {
        let encoded = STANDARD.encode(format!("{}:api_token", self.api_token));
        format!("Basic {}", encoded)
    }
    
    /// 共通のAPIリクエスト関数
    pub async fn api_request<T: serde::de::DeserializeOwned>(
        &self, 
        method: reqwest::Method, 
        url: &str, 
        body: Option<serde_json::Value>
    ) -> Result<T> {
        let mut req = self.client.request(method.clone(), url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json");
            
        if let Some(json_body) = body {
            req = req.json(&json_body);
        }
        
        debug!("APIリクエスト送信: {} {}", method, url);
        
        let response = match req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                let details = if e.is_timeout() {
                    "タイムアウトエラー"
                } else if e.is_connect() {
                    "接続エラー"
                } else if e.is_request() {
                    "リクエスト作成エラー"
                } else {
                    "不明なエラー"
                };
                error!("Toggl APIリクエスト失敗 ({}): {} - {}", url, details, e);
                return Err(anyhow!("APIリクエスト送信に失敗: {} - {}", details, e));
            }
        };
            
        let status = response.status();
        
        if status.is_success() {
            debug!("APIレスポンス成功: {} {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown Status"));
            match response.json::<T>().await {
                Ok(data) => Ok(data),
                Err(e) => {
                    error!("レスポンスのJSONパースに失敗: {} - {}", url, e);
                    Err(anyhow!("JSONパースエラー: {}", e))
                }
            }
        } else {
            let err_text = match response.text().await {
                Ok(text) => text,
                Err(_) => "レスポンステキストの取得に失敗".to_string()
            };
            
            error!("Toggl APIエラー: HTTP {} {} - {}", 
                status.as_u16(), 
                status.canonical_reason().unwrap_or("Unknown Status"),
                err_text
            );
            
            Err(anyhow!("APIエラー ({}): HTTP {} {} - {}", 
                url, 
                status.as_u16(), 
                status.canonical_reason().unwrap_or("Unknown Status"),
                err_text
            ))
        }
    }
    
    /// ワークスペース一覧を取得
    pub async fn get_workspaces(&self) -> Result<Vec<Workspace>> {
        self.api_request::<Vec<Workspace>>(
            reqwest::Method::GET,
            "https://api.track.toggl.com/api/v9/me/workspaces",
            None
        ).await
    }
    
    /// プロジェクト一覧を取得
    pub async fn get_projects(&self) -> Result<Vec<Project>> {
        // ワークスペースIDを取得
        let workspaces = self.get_workspaces().await?;
        if workspaces.is_empty() {
            bail!("ワークスペースが見つかりません");
        }
        
        let workspace_id = workspaces[0].id;
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/projects", workspace_id);
        
        self.api_request::<Vec<Project>>(
            reqwest::Method::GET,
            &url,
            None
        ).await
    }
    
    /// 時間エントリを作成
    pub async fn create_time_entry(&self, wid: u64, entry: &TimeEntry) -> Result<u64> {
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/time_entries", wid);
        
        // 詳細なデバッグ情報のためのJSON文字列
        debug!("Raw TimeEntry object: {:?}", entry);
        let json_string = match serde_json::to_string_pretty(&entry) {
            Ok(json) => {
                debug!("Raw JSON request: {}", json);
                json
            }
            Err(e) => {
                error!("Failed to serialize request to JSON: {}", e);
                return Err(anyhow::anyhow!("Failed to serialize request"));
            }
        };
        
        // リクエストのfieldごとの詳細な値確認
        debug!("TimeEntry fields:");
        debug!("  - description: {}", entry.description);
        debug!("  - wid: {}", entry.wid);
        debug!("  - pid: {:?}", entry.pid);
        debug!("  - start: {}", entry.start);
        debug!("  - stop: {:?}", entry.stop);
        debug!("  - duration: {:?}", entry.duration);
        
        // 共通APIリクエスト関数を使用
        let response: serde_json::Value = self.api_request(
            reqwest::Method::POST,
            &url,
            Some(serde_json::from_str(&json_string).unwrap())
        ).await?;
        
        // IDを抽出
        let id = response["id"].as_u64().ok_or_else(|| anyhow::anyhow!("No ID in response"))?;
        debug!("Created time entry with ID: {}", id);
        
        Ok(id)
    }
    
    /// プロジェクト名からプロジェクトを検索
    pub async fn find_project_by_name(&self, name: &str) -> Result<Option<Project>> {
        let projects = self.get_projects().await?;
        
        for project in projects {
            if project.name == name {
                return Ok(Some(project));
            }
        }
        
        Ok(None)
    }
    
    /// 指定期間の時間エントリ一覧を取得
    pub async fn get_time_entries(
        &self,
        workspace_id: u64,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<Vec<crate::event::TogglTimeEntry>> {
        let start_date_fmt = format_datetime(&start_date);
        let end_date_fmt = format_datetime(&end_date);
        let start_date_str = urlencoding::encode(&start_date_fmt);
        let end_date_str = urlencoding::encode(&end_date_fmt);
        let url = format!(
            "https://api.track.toggl.com/api/v9/workspaces/{}/time_entries?start_date={}&end_date={}",
            workspace_id, start_date_str, end_date_str
        );

        self.api_request(reqwest::Method::GET, &url, None).await
    }
}

/// RFC3339形式の日付文字列を生成（Toggl API用にナノ秒を除去）
pub fn format_datetime(dt: &DateTime<Utc>) -> String {
    // ISO 8601 形式を使用して、Toggl APIの要求に合わせる
    // Zを使ってUTC（GMT）であることを示す
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// シンプルな時間エントリ作成ヘルパー関数
pub async fn create_simple_time_entry(
    api_token: &str,
    workspace_id: u64,
    description: &str,
    start_time: &DateTime<Utc>,
    stop_time: &DateTime<Utc>,
) -> Result<u64> {
    let client = TogglClient::new(api_token.to_string());
    
    let time_entry = TimeEntry {
        description: description.to_string(),
        wid: workspace_id,
        pid: None,
        start: format_datetime(start_time),
        stop: Some(format_datetime(stop_time)),
        duration: Some((stop_time.clone() - start_time.clone()).num_seconds()),
        tags: None,
        created_with: Some("toggl_linux_rs".to_string()),
        metadata: None,
    };
    
    client.create_time_entry(workspace_id, &time_entry).await
} 
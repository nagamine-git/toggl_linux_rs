use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Timelike, Utc};
use log::{info, debug};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use base64::Engine;

use crate::analysis::AnalysisResult;
use crate::config::AppConfig;

/// Togglのプロジェクト情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TogglProject {
    /// プロジェクトID
    pub id: u64,
    
    /// プロジェクト名
    pub name: String,
    
    /// ワークスペースID
    pub wid: u64,
    
    /// クライアントID（オプション）
    pub cid: Option<u64>,
}

/// Togglのタイムエントリ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeEntry {
    /// 説明
    pub description: String,
    
    /// ワークスペースID
    pub wid: u64,
    
    /// プロジェクトID（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_with: Option<String>,
    
    /// イベントメタデータ（Toggl APIのv9で追加されたフィールド）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_metadata: Option<serde_json::Value>,
}

/// TimeEntryの作成リクエスト
#[derive(Debug, Serialize)]
struct CreateTimeEntryRequest {
    time_entry: TimeEntry,
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

/// プロジェクト一覧レスポンス
#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    #[serde(default)]
    data: Vec<TogglProject>,
}

/// Toggl タイムエントリ情報
#[derive(Debug, Deserialize)]
pub struct TogglTimeEntry {
    pub id: u64,
    pub workspace_id: u64,
    pub project_id: Option<u64>,
    pub description: String,
    pub start: String,
    pub stop: Option<String>,
    pub duration: i64,
    pub tags: Option<Vec<String>>,
}

/// Toggl ワークスペース情報
#[derive(Debug, Deserialize)]
pub struct TogglWorkspace {
    pub id: u64,
    pub name: String,
    pub organization_id: u64,
}

/// TogglのAPIクライアント
pub struct TogglClient {
    api_token: String,
    client: reqwest::Client,
    workspace_id: u64,
}

impl TogglClient {
    /// 新しいTogglクライアントを作成
    pub fn new(api_token: &str, workspace_id: u64) -> Self {
        let client = reqwest::Client::new();
        
        TogglClient {
            client,
            api_token: api_token.to_string(),
            workspace_id,
        }
    }
    
    /// 認証用ヘッダーを作成
    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        
        let auth = format!("{}:api_token", self.api_token);
        let auth_value = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(auth));
        
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(&auth_value).unwrap(),
        );
        
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        
        headers
    }
    
    /// Togglのワークスペース一覧を取得
    pub async fn get_workspaces(&self) -> Result<Vec<TogglWorkspace>> {
        let url = "https://api.track.toggl.com/api/v9/workspaces";
        
        let response = self.client
            .get(url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to retrieve workspaces")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to retrieve workspaces: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        let workspaces: Vec<TogglWorkspace> = response
            .json()
            .await
            .context("Failed to parse workspace response")?;
        
        Ok(workspaces)
    }
    
    /// ワークスペースのプロジェクト一覧を取得
    pub async fn get_projects(&self) -> Result<Vec<TogglProject>> {
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/projects", self.workspace_id);
        
        let response = self.client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to retrieve projects")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to retrieve projects: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        let projects: Vec<TogglProject> = response
            .json()
            .await
            .context("Failed to parse projects response")?;
        
        Ok(projects)
    }
    
    /// 新しいタイムエントリを作成
    pub async fn create_time_entry(&self, entry: TimeEntry) -> Result<u64> {
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/time_entries", self.workspace_id);
        
        // v9 APIではリクエストボディ形式が変更されているため、直接JSONを構築
        let request_body = serde_json::json!({
            "description": entry.description,
            "project_id": entry.pid,
            "start": entry.start,
            "stop": entry.stop,
            "duration": entry.duration,
            "tags": entry.tags,
            "created_with": entry.created_with,
            "workspace_id": entry.wid,
            "event_metadata": entry.event_metadata
        });
        
        let response = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&request_body)
            .send()
            .await
            .context("Failed to send time entry request")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to create time entry: HTTP status {}, {}",
                status,
                err_text
            ));
        }
        
        // v9 APIではレスポンス形式が変更されているため、直接IDを抽出
        let time_entry: TogglTimeEntry = response.json().await
            .context("Failed to parse time entry response")?;
        
        Ok(time_entry.id)
    }
    
    /// プロジェクト名からIDを検索
    pub async fn find_project_by_name(&self, name: &str) -> Result<Option<u64>> {
        let projects = self.get_projects().await?;
        
        for project in projects {
            if project.name.to_lowercase() == name.to_lowercase() {
                return Ok(Some(project.id));
            }
        }
        
        Ok(None)
    }

    /// 実行中のタイムエントリを取得
    pub async fn get_running_time_entry(&self) -> Result<Option<TogglTimeEntry>> {
        let url = "https://api.track.toggl.com/api/v9/me/time_entries/current";
        
        let response = self.client
            .get(url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to retrieve current time entry")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to retrieve current time entry: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        // ステータスコードが204の場合は実行中のエントリがない
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        
        let time_entry: TogglTimeEntry = response
            .json()
            .await
            .context("Failed to parse time entry response")?;
        
        Ok(Some(time_entry))
    }

    pub async fn start_time_entry(&self, project_id: u64, description: &str) -> Result<TogglTimeEntry> {
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/time_entries", self.workspace_id);
        
        let now = Utc::now();
        let body = serde_json::json!({
            "created_with": "toggl_linux_rs",
            "description": description,
            "project_id": project_id,
            "start": now.to_rfc3339(),
            "workspace_id": self.workspace_id,
        });
        
        let response = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await
            .context("Failed to start time entry")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to start time entry: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        let time_entry: TogglTimeEntry = response
            .json()
            .await
            .context("Failed to parse time entry response")?;
        
        Ok(time_entry)
    }

    pub async fn get_time_entries(&self, start_date: &DateTime<Utc>, end_date: &DateTime<Utc>) -> Result<Vec<TogglTimeEntry>> {
        let url = format!(
            "https://api.track.toggl.com/api/v9/me/time_entries?start_date={}&end_date={}",
            start_date.to_rfc3339(),
            end_date.to_rfc3339()
        );
        
        let response = self.client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to retrieve time entries")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to retrieve time entries: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        let time_entries: Vec<TogglTimeEntry> = response
            .json()
            .await
            .context("Failed to parse time entries response")?;
        
        Ok(time_entries)
    }

    pub async fn stop_time_entry(&self, time_entry_id: u64) -> Result<TogglTimeEntry> {
        let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/time_entries/{}/stop", self.workspace_id, time_entry_id);
        
        let response = self.client
            .patch(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("Failed to stop time entry")?;
        
        // レスポンスステータスのチェック
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to stop time entry: HTTP status {}, response: {}",
                status,
                err_text
            ));
        }
        
        let time_entry: TogglTimeEntry = response
            .json()
            .await
            .context("Failed to parse time entry response")?;
        
        Ok(time_entry)
    }
}

/// プロジェクトIDを推論する
async fn infer_project_id(
    toggl_client: &TogglClient, 
    analysis: &AnalysisResult
) -> Result<Option<u64>> {
    debug!("プロジェクトID推論開始");
    
    // プロジェクト一覧を取得
    let projects = toggl_client.get_projects().await?;
    debug!("取得したプロジェクト数: {}", projects.len());
    
    // 全プロジェクト一覧をデバッグ出力
    debug!("利用可能なプロジェクト一覧:");
    for (i, project) in projects.iter().enumerate() {
        debug!("  {}. {} (ID: {})", i+1, project.name, project.id);
    }
    
    // プロジェクト推論データを収集
    let mut match_candidates = Vec::new();
    
    // 活動名を小文字に変換
    let activity_lower = analysis.activity.to_lowercase();
    
    // ウィンドウタイトルの情報を取得（あれば）
    let window_title_lower = analysis.window_title
        .as_ref()
        .map(|wt| wt.to_lowercase());
    
    // カレンダーイベントの情報を取得（あれば）
    let calendar_title_lower = analysis.calendar_event
        .as_ref()
        .map(|event| event.title.to_lowercase());
    
    debug!("推論に使用する情報:");
    debug!("- 活動名: {}", activity_lower);
    if let Some(ref wt) = window_title_lower {
        debug!("- ウィンドウタイトル: {}", wt);
    }
    if let Some(ref ct) = calendar_title_lower {
        debug!("- カレンダーイベント: {}", ct);
    }
    
    // 各プロジェクトとの類似度を計算
    for project in &projects {
        let project_name_lower = project.name.to_lowercase();
        let mut score = 0.0;
        let mut match_reasons = Vec::new();
        
        // 1. 完全一致の場合 (highest priority)
        if project_name_lower == activity_lower {
            score = 1.0;
            match_reasons.push(format!("活動名と完全一致"));
        }
        // 2. 部分文字列マッチング
        else if project_name_lower.contains(&activity_lower) {
            score = 0.8;
            match_reasons.push(format!("プロジェクト名が活動名を含む"));
        }
        else if activity_lower.contains(&project_name_lower) {
            score = 0.7;
            match_reasons.push(format!("活動名がプロジェクト名を含む"));
        }
        // 3. 単語レベルでの一致を検出
        else {
            // 単語に分割して一致を検索
            let project_words: Vec<&str> = project_name_lower.split_whitespace().collect();
            let activity_words: Vec<&str> = activity_lower.split_whitespace().collect();
            
            let mut matching_words = 0;
            for pword in &project_words {
                if activity_words.contains(pword) {
                    matching_words += 1;
                }
            }
            
            if matching_words > 0 {
                // 一致する単語の割合でスコア付け
                let word_match_score = matching_words as f64 / project_words.len().max(1) as f64;
                score = word_match_score * 0.6; // 単語一致は完全一致より低い優先度
                match_reasons.push(format!("{}個の単語が一致", matching_words));
            }
        }
        
        // 4. ウィンドウタイトルを考慮
        if let Some(ref window_title) = window_title_lower {
            if window_title.contains(&project_name_lower) {
                score += 0.2;
                match_reasons.push(format!("ウィンドウタイトルがプロジェクト名を含む"));
            }
        }
        
        // 5. カレンダーイベントを考慮
        if let Some(ref calendar_title) = calendar_title_lower {
            if calendar_title.contains(&project_name_lower) {
                score += 0.3;
                match_reasons.push(format!("カレンダーイベントがプロジェクト名を含む"));
            }
        }
        
        // 有意義なスコアがあれば候補に追加
        if score > 0.0 {
            match_candidates.push((project.id, project.name.clone(), score, match_reasons));
        }
    }
    
    // スコアの高い順にソート
    match_candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    
    // 候補をログに出力
    debug!("プロジェクト候補リスト:");
    for (i, (id, name, score, reasons)) in match_candidates.iter().enumerate() {
        debug!("候補{}: {} (ID: {}, スコア: {:.2})", i+1, name, id, score);
        for reason in reasons {
            debug!("  - {}", reason);
        }
    }
    
    // 最良の候補を返す（スコアが閾値以上の場合）
    if !match_candidates.is_empty() && match_candidates[0].2 >= 0.5 {
        let best_match = &match_candidates[0];
        info!("選択されたプロジェクト: {} (ID: {}, スコア: {:.2})", 
              best_match.1, best_match.0, best_match.2);
        Ok(Some(best_match.0))
    } else {
        debug!("適切なプロジェクトが見つかりませんでした");
        Ok(None)
    }
}

/// 文字列を安全に切り詰める（UTF-8文字境界を保持）
fn truncate_string_safely(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    
    let mut char_indices = s.char_indices();
    let mut last_valid_index = 0;
    
    // 文字の境界を保持しながら最大長に近い位置を探す
    while let Some((idx, _)) = char_indices.next() {
        if idx > max_len {
            break;
        }
        last_valid_index = idx;
    }
    
    // 切り詰めた文字列に「...」を追加
    format!("{}...", &s[..last_valid_index])
}

/// ユーザーに活動候補を提示する（コマンドライン用）
pub fn present_activity_choices(analysis: &AnalysisResult) -> Result<String> {
    println!("活動推定の確度が低いため、以下から選択してください：");
    println!("0: [{}] (確度: {:.2})", analysis.activity, analysis.confidence);
    
    for (i, alt) in analysis.alternatives.iter().enumerate() {
        println!(
            "{}: [{}] (確度: {:.2})",
            i + 1,
            alt.activity,
            alt.confidence
        );
    }
    
    println!("{}: [新規活動を入力]", analysis.alternatives.len() + 1);
    
    // 標準入力から選択を受け取る
    print!("選択（数字）: ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    let choice = input.trim().parse::<usize>().unwrap_or(0);
    
    if choice == 0 {
        return Ok(analysis.activity.clone());
    } else if choice <= analysis.alternatives.len() {
        return Ok(analysis.alternatives[choice - 1].activity.clone());
    } else {
        print!("新しい活動名を入力: ");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        
        let mut new_activity = String::new();
        std::io::stdin().read_line(&mut new_activity)?;
        
        return Ok(new_activity.trim().to_string());
    }
}

/// main.rsの分析関数から呼び出す用の関数
pub async fn register_to_toggl(config: &AppConfig, analysis: &AnalysisResult) -> Result<()> {
    // Togglクライアントの初期化
    let toggl_client = TogglClient::new(
        &config.toggl.api_token,
        config.toggl.workspace_id,
    );
    
    // 時間ブロック設定を取得
    let time_block_division = config.general.time_block_division;
    let minutes_per_block = 60 / time_block_division as u64;
    
    // 活動の開始・終了時刻を決定（時間はタイムブロック単位に切り捨て）
    let start_time = analysis.timestamp
        .with_minute(analysis.timestamp.minute() / minutes_per_block as u32 * minutes_per_block as u32)
        .unwrap_or(analysis.timestamp)
        .with_second(0)
        .unwrap_or(analysis.timestamp);
    
    let stop_time = start_time + Duration::minutes(minutes_per_block as i64);
    
    // プライベートブラウジングのフラグを確認（AnalysisResultに含まれていない場合はfalse）
    let is_private_browsing = analysis.window_title
        .as_ref()
        .map(|title| title.to_lowercase().contains("private") || title.to_lowercase().contains("incognito"))
        .unwrap_or(false);
    
    // ActivityResultにis_private_browsingフィールドが無いため、拡張AnalysisResultを作成
    let extended_analysis = ExtendedAnalysisResult {
        base: analysis,
        is_private_browsing,
    };
    
    // 詳細なRegister to Toggl関数を呼び出す
    register_to_toggl_impl(
        &toggl_client,
        &extended_analysis,
        config.toggl.workspace_id,
        start_time,
        stop_time,
        true, // デフォルトでプライベートブラウジングはスキップ
    ).await
}

// AnalysisResultを拡張して必要なフィールドを追加
struct ExtendedAnalysisResult<'a> {
    base: &'a AnalysisResult,
    is_private_browsing: bool,
}

/// 活動記録をTogglに登録する（内部実装）
async fn register_to_toggl_impl(
    toggl_client: &TogglClient,
    analysis: &ExtendedAnalysisResult<'_>,
    workspace_id: u64,
    start_time: DateTime<Utc>,
    stop_time: DateTime<Utc>,
    should_skip_private: bool,
) -> Result<()> {
    // プライベートブラウジングは記録しない設定の場合はスキップ
    if should_skip_private && analysis.is_private_browsing {
        info!("プライベートブラウジング中の活動はスキップします");
        return Ok(());
    }

    // 活動の信頼度が低い場合もスキップ
    if analysis.base.confidence < 0.5 {
        info!("活動の信頼度が低いためスキップします: {:.2}", analysis.base.confidence);
        return Ok(());
    }

    debug!("Togglに記録を開始: {}", analysis.base.activity);
    debug!("開始時間: {}", start_time.to_rfc3339());
    debug!("終了時間: {}", stop_time.to_rfc3339());
    debug!("信頼度: {:.2}", analysis.base.confidence);
    
    if let Some(ref window_title) = analysis.base.window_title {
        debug!("ウィンドウタイトル: {}", window_title);
    }
    
    if let Some(ref calendar_event) = analysis.base.calendar_event {
        debug!("カレンダーイベント: {}", calendar_event.title);
    }

    // プロジェクトIDの推論
    let project_id = infer_project_id(toggl_client, analysis.base).await?;
    if let Some(id) = project_id {
        debug!("プロジェクトID: {}", id);
    } else {
        debug!("プロジェクトID: なし");
    }

    // 直前のエントリを取得して同名エントリの有無を確認（マージ処理）
    let one_hour_ago = start_time - Duration::hours(1);
    debug!("直前のエントリ検索中 (期間: {} ～ {})", one_hour_ago.to_rfc3339(), start_time.to_rfc3339());
    
    match toggl_client.get_time_entries(&one_hour_ago, &start_time).await {
        Ok(entries) => {
            if !entries.is_empty() {
                debug!("直前の時間エントリ数: {}", entries.len());
                
                // 直前のエントリを逆順（新しいものから）でチェック
                for entry in entries.iter().rev() {
                    debug!("エントリ確認: {} (開始: {}, 終了: {:?})", 
                           entry.description, entry.start, entry.stop);
                    
                    // 同じ説明文かつストップ時間が開始時間の近く（15分以内）なら連結候補
                    if entry.description == analysis.base.activity && entry.stop.is_some() {
                        if let Ok(last_stop) = chrono::DateTime::parse_from_rfc3339(&entry.stop.clone().unwrap()) {
                            let last_stop_utc = last_stop.with_timezone(&chrono::Utc);
                            let minutes_diff = (start_time - last_stop_utc).num_minutes();
                            
                            debug!("前回終了時間との差: {}分", minutes_diff);
                            
                            // 15分以内の同名エントリならマージ
                            if minutes_diff.abs() <= 15 {
                                info!("同名の直前エントリをマージします (ID: {})", entry.id);
                                
                                // マージ用のJSONボディを構築
                                let update_body = serde_json::json!({
                                    "stop": stop_time.to_rfc3339(),
                                });
                                
                                // エントリを更新
                                let url = format!("https://api.track.toggl.com/api/v9/workspaces/{}/time_entries/{}", 
                                                 workspace_id, entry.id);
                                
                                match toggl_client.client.patch(&url)
                                    .headers(toggl_client.auth_headers())
                                    .json(&update_body)
                                    .send()
                                    .await {
                                    Ok(response) => {
                                        let status = response.status();
                                        if status.is_success() {
                                            info!("タイムエントリを更新しました (ID: {})", entry.id);
                                            return Ok(());
                                        } else {
                                            let err_text = response.text().await.unwrap_or_default();
                                            debug!("エントリ更新失敗: HTTP {} {}", status, err_text);
                                        }
                                    },
                                    Err(e) => {
                                        debug!("エントリ更新リクエスト失敗: {}", e);
                                    }
                                }
                                
                                // マージ試行後は処理を続行（失敗しても新規エントリを作成）
                                break;
                            }
                        }
                    }
                }
            }
        },
        Err(e) => {
            debug!("直前のエントリ取得に失敗: {}", e);
        }
    }

    // TimeEntryリクエストの作成（マージできない場合は新規作成）
    let time_entry = TimeEntry {
        description: analysis.base.activity.clone(),
        wid: workspace_id,
        pid: project_id,
        start: start_time.to_rfc3339(),
        stop: Some(stop_time.to_rfc3339()),
        duration: Some((stop_time - start_time).num_seconds()),
        created_with: Some("toggl_linux_rs".to_string()),
        tags: None, // タグを削除
        event_metadata: Some(serde_json::json!({
            "origin_feature": "linux_rs_activity",
            "visible_goals_count": 0
        })),
    };

    // 時間記録の登録
    debug!("Togglへ時間記録を送信...");
    let entry_id = toggl_client.create_time_entry(time_entry).await?;
    info!("Togglへの時間記録を完了しました (ID: {})", entry_id);

    Ok(())
} 
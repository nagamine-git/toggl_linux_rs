use anyhow::{Context, Result};
use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select, MultiSelect};
use std::fs;
use std::net::TcpListener;
use std::io::{Read, Write};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::event::TogglClient;

const REDIRECT_URI: &str = "http://localhost:8080";
const OAUTH_SCOPES: &str = "https://www.googleapis.com/auth/calendar.readonly";

#[derive(Debug, Serialize, Deserialize)]
struct GoogleOAuthToken {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleCalendarListResponse {
    items: Vec<GoogleCalendar>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleCalendar {
    id: String,
    summary: String,
    description: Option<String>,
    primary: Option<bool>,
}

/// 対話型設定ウィザード
pub struct ConfigWizard {
    term: Term,
    theme: ColorfulTheme,
}

impl ConfigWizard {
    /// 新しいウィザードインスタンスを作成
    pub fn new() -> Self {
        Self {
            term: Term::stdout(),
            theme: ColorfulTheme::default(),
        }
    }

    /// ウィザードを実行
    pub async fn run(&self) -> Result<()> {
        self.term.clear_screen()?;
        
        println!("{}", style("toggl_linux_rs 設定ウィザード").bold().underlined());
        println!("このウィザードでは、アプリケーションの設定を対話的に行います。\n");
        
        // 基本設定
        let general_config = self.configure_general()?;
        
        // Toggl設定
        let toggl_config = self.configure_toggl().await?;
        
        // OpenAI設定
        let openai_config = self.configure_openai()?;
        
        // Googleカレンダー設定（オプション）
        let google_config = self.configure_google_calendar().await?;
        
        // 設定をマージ
        let config = AppConfig {
            general: general_config,
            toggl: toggl_config,
            openai: Some(openai_config),
            google_calendar: google_config,
        };
        
        // 設定ファイルを保存
        self.save_config(&config)?;
        
        println!("\n{}", style("設定が完了しました！").green().bold());
        println!("アプリケーションを実行するには: {} を実行してください", style("cargo run").cyan());
        
        Ok(())
    }
    
    /// 基本設定
    fn configure_general(&self) -> Result<crate::config::GeneralConfig> {
        println!("\n{}", style("基本設定").bold());
        
        // 設定値を入力
        let data_dir: String = Input::with_theme(&self.theme)
            .with_prompt("データ保存ディレクトリのパスを入力してください")
            .default("./data".into())
            .interact_text()?;
        
        // 他の設定値は直接デフォルト値を使用
        Ok(crate::config::GeneralConfig {
            data_dir,
            confidence_threshold: 0.5,
            collect_interval_secs: 60,
            time_block_division: 4,
            idle_threshold_secs: 300, // デフォルトは5分
        })
    }
    
    /// Toggl設定
    async fn configure_toggl(&self) -> Result<crate::config::TogglConfig> {
        println!("\n{}", style("Toggl設定").bold());
        println!("Toggl APIトークンは、https://track.toggl.com/profile で取得できます。");
        
        // APIトークン入力
        let api_token: String = Input::with_theme(&self.theme)
            .with_prompt("Toggl APIトークン")
            .interact_on(&self.term)?;
        
        // ワークスペース選択
        println!("\nTogglワークスペース一覧を取得中...");
        
        let client = TogglClient::new(&api_token, 0); // ダミーのワークスペースID
        
        match client.get_workspaces().await {
            Ok(workspaces) => {
                if workspaces.is_empty() {
                    println!("ワークスペースが見つかりませんでした。");
                    // デフォルト値を設定
                    Ok(crate::config::TogglConfig {
                        api_token,
                        workspace_id: 0,
                    })
                } else {
                    let workspace_names: Vec<String> = workspaces
                        .iter()
                        .map(|w| format!("{} (ID: {})", w.name, w.id))
                        .collect();
                    
                    let selection = Select::with_theme(&self.theme)
                        .with_prompt("使用するワークスペースを選択してください")
                        .default(0)
                        .items(&workspace_names)
                        .interact_on(&self.term)?;
                    
                    let selected_workspace = &workspaces[selection];
                    println!("選択されたワークスペース: {}", style(&selected_workspace.name).green());
                    
                    Ok(crate::config::TogglConfig {
                        api_token,
                        workspace_id: selected_workspace.id,
                    })
                }
            }
            Err(e) => {
                println!("ワークスペース一覧の取得に失敗しました: {}", e);
                // 手動入力に切り替え
                let workspace_id: u64 = Input::with_theme(&self.theme)
                    .with_prompt("ワークスペースID（手動入力）")
                    .interact_on(&self.term)?;
                
                Ok(crate::config::TogglConfig {
                    api_token,
                    workspace_id,
                })
            }
        }
    }
    
    /// OpenAI設定
    fn configure_openai(&self) -> Result<crate::config::OpenAIConfig> {
        println!("\n{}", style("OpenAI設定").bold());
        println!("OpenAIのAPIキーは、https://platform.openai.com/api-keys で取得できます。");
        
        // APIキー入力
        let api_key: String = Input::with_theme(&self.theme)
            .with_prompt("OpenAI APIキー")
            .interact_on(&self.term)?;
        
        // モデル選択
        let models = vec![
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4",
            "gpt-3.5-turbo",
        ];
        
        let selection = Select::with_theme(&self.theme)
            .with_prompt("使用するモデルを選択してください")
            .default(0)
            .items(&models)
            .interact_on(&self.term)?;
        
        let model = models[selection].to_string();
        
        Ok(crate::config::OpenAIConfig {
            api_key,
            model,
        })
    }
    
    /// Googleカレンダー設定（オプション）
    async fn configure_google_calendar(&self) -> Result<Option<crate::config::GoogleCalendarConfig>> {
        println!("\n{}", style("Google Calendar設定（オプション）").bold());
        
        let use_google_calendar = Confirm::with_theme(&self.theme)
            .with_prompt("Google Calendarと連携しますか？")
            .default(false)
            .interact_on(&self.term)?;
        
        if !use_google_calendar {
            return Ok(None);
        }
        
        println!("Google Cloud Consoleでの準備が必要です：");
        println!("1. https://console.cloud.google.com/apis/dashboard で新しいプロジェクトを作成");
        println!("2. Google Calendar APIを有効化");
        println!("3. OAuth同意画面を設定（テスト用は外部を選択）");
        println!("4. OAuth 2.0クライアントIDを作成（リダイレクトURIに {} を追加）", REDIRECT_URI);
        println!("");
        
        // クライアントID
        let client_id: String = Input::with_theme(&self.theme)
            .with_prompt("Google Cloud OAuth クライアントID")
            .interact_on(&self.term)?;
        
        // クライアントシークレット
        let client_secret: String = Input::with_theme(&self.theme)
            .with_prompt("Google Cloud OAuth クライアントシークレット")
            .interact_on(&self.term)?;
        
        // OAuth認証フローを実行
        println!("\n{}", style("OAuth認証を開始します...").green());
        
        // 認証コードを取得
        let auth_code = self.get_oauth_authorization_code(&client_id)?;
        
        // 認証コードをトークンに交換
        let token = self.exchange_auth_code_for_token(
            &client_id, 
            &client_secret, 
            &auth_code
        ).await?;
        
        println!("{}", style("認証が完了しました！").green());
        
        // カレンダー一覧を取得
        println!("カレンダー一覧を取得しています...");
        let calendars = self.get_calendar_list(&token.access_token).await?;
        
        let calendar_items: Vec<String> = calendars.items
            .iter()
            .map(|cal| {
                let primary_label = if cal.primary.unwrap_or(false) { " (主カレンダー)" } else { "" };
                format!("{}{} (ID: {})", cal.summary, primary_label, cal.id)
            })
            .collect();
        
        // 主カレンダーのインデックスを見つける
        let default_selections: Vec<bool> = calendars.items
            .iter()
            .map(|cal| cal.primary.unwrap_or(false))
            .collect();
        
        // 主カレンダーが見つからない場合は最初の項目を選択
        let default_selections = if default_selections.iter().any(|&x| x) || calendar_items.is_empty() {
            default_selections
        } else {
            // 何も選択されていない場合は最初の項目を選択
            calendar_items.iter()
                .enumerate()
                .map(|(i, _)| i == 0)
                .collect()
        };
        
        // 複数選択
        let selected = MultiSelect::with_theme(&self.theme)
            .with_prompt("使用するカレンダーを選択してください（スペースキーで選択、Enterで確定）")
            .items(&calendar_items)
            .defaults(&default_selections)
            .interact_on(&self.term)?;
        
        if selected.is_empty() {
            println!("カレンダーが選択されていません。主カレンダーを使用します。");
            
            // 主カレンダーを探す
            let primary_calendar = calendars.items.iter()
                .find(|cal| cal.primary.unwrap_or(false))
                .map(|cal| cal.id.clone())
                .unwrap_or_else(|| "primary".to_string());
            
            return Ok(Some(crate::config::GoogleCalendarConfig {
                client_id,
                client_secret,
                refresh_token: token.refresh_token,
                calendar_ids: primary_calendar,
            }));
        }
        
        // 選択されたカレンダーIDをカンマ区切りで連結
        let calendar_ids = selected.iter()
            .map(|&idx| calendars.items[idx].id.clone())
            .collect::<Vec<String>>()
            .join(",");
        
        println!("{}", style("選択されたカレンダー:").green());
        for &idx in &selected {
            println!(" - {}", calendars.items[idx].summary);
        }
        
        Ok(Some(crate::config::GoogleCalendarConfig {
            client_id,
            client_secret,
            refresh_token: token.refresh_token,
            calendar_ids,
        }))
    }
    
    /// OAuth認証コードを取得
    fn get_oauth_authorization_code(&self, client_id: &str) -> Result<String> {
        // 認証URLを構築
        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/auth?client_id={}&redirect_uri={}&scope={}&response_type=code&access_type=offline&prompt=consent",
            client_id,
            urlencoding::encode(REDIRECT_URI),
            urlencoding::encode(OAUTH_SCOPES)
        );
        
        println!("ブラウザでGoogle認証ページを開きます...");
        
        // ブラウザで認証URLを開く
        if let Err(e) = open::that(&auth_url) {
            println!("ブラウザを自動で開けませんでした: {}", e);
            println!("以下のURLをブラウザで開いて認証を行ってください:");
            println!("{}", auth_url);
        }
        
        // ローカルサーバーを起動してリダイレクトを待機
        println!("Google認証ページでログインして、アクセスを許可してください...");
        
        let listener = TcpListener::bind("127.0.0.1:8080").context("ローカルサーバーの起動に失敗しました")?;
        
        // 最初の接続を受け入れる
        let (mut stream, _) = listener.accept().context("リダイレクト待機中にエラーが発生しました")?;
        
        // リクエストを読み取る
        let mut buffer = [0; 1024];
        stream.read(&mut buffer).context("リクエストの読み取りに失敗しました")?;
        
        // リクエストからcodeパラメータを抽出
        let request = String::from_utf8_lossy(&buffer[..]);
        let uri = request.lines().next()
            .ok_or_else(|| anyhow::anyhow!("リクエストの解析に失敗しました"))?
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("URLの解析に失敗しました"))?;
        
        let url = Url::parse(&format!("http://localhost{}", uri))
            .context("URLの解析に失敗しました")?;
        
        let code = url.query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string())
            .ok_or_else(|| anyhow::anyhow!("認証コードが見つかりませんでした"))?;
        
        // 成功ページを返す
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><h1>認証成功</h1><p>このウィンドウを閉じて、アプリケーションに戻ってください。</p></body></html>";
        stream.write_all(response.as_bytes()).context("レスポンスの送信に失敗しました")?;
        
        Ok(code)
    }
    
    /// 認証コードをトークンに交換
    async fn exchange_auth_code_for_token(
        &self,
        client_id: &str,
        client_secret: &str,
        auth_code: &str
    ) -> Result<GoogleOAuthToken> {
        let client = reqwest::Client::new();
        
        let params = [
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", auth_code),
            ("redirect_uri", REDIRECT_URI),
            ("grant_type", "authorization_code"),
        ];
        
        let response = client.post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .context("トークン交換リクエストの送信に失敗しました")?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "エラー詳細を取得できませんでした".to_string());
            return Err(anyhow::anyhow!("トークン交換に失敗しました: {}", error_text));
        }
        
        let token: GoogleOAuthToken = response.json().await
            .context("トークンレスポンスの解析に失敗しました")?;
        
        Ok(token)
    }
    
    /// カレンダー一覧を取得
    async fn get_calendar_list(&self, access_token: &str) -> Result<GoogleCalendarListResponse> {
        let client = reqwest::Client::new();
        
        let response = client.get("https://www.googleapis.com/calendar/v3/users/me/calendarList")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await
            .context("カレンダー一覧の取得に失敗しました")?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "エラー詳細を取得できませんでした".to_string());
            return Err(anyhow::anyhow!("カレンダー一覧の取得に失敗しました: {}", error_text));
        }
        
        let calendar_list: GoogleCalendarListResponse = response.json().await
            .context("カレンダー一覧の解析に失敗しました")?;
        
        Ok(calendar_list)
    }
    
    /// 設定ファイルを保存
    fn save_config(&self, config: &AppConfig) -> Result<()> {
        println!("\n設定内容を確認します：");
        
        // 設定内容のプレビュー
        let config_str = toml::to_string_pretty(config)?;
        println!("{}", style("```").dim());
        println!("{}", config_str);
        println!("{}", style("```").dim());
        
        let confirm = Confirm::with_theme(&self.theme)
            .with_prompt("この設定をconfig.tomlに保存しますか？")
            .default(true)
            .interact_on(&self.term)?;
        
        if confirm {
            fs::write("config.toml", config_str)
                .context("設定ファイルの保存に失敗しました")?;
            println!("設定ファイルを {} に保存しました", style("config.toml").yellow());
            Ok(())
        } else {
            println!("設定の保存をキャンセルしました");
            Ok(())
        }
    }
} 
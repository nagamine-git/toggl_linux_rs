use anyhow::{Context, Result};
use serde::Deserialize;
use serde::Serialize;
use std::fs::read_to_string;
use std::path::Path;

/// アプリケーション全体の設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// アプリケーション全般の設定
    pub general: GeneralConfig,
    
    /// Toggl API の設定
    pub toggl: TogglConfig,
    
    /// OpenAI API の設定（オプション）
    pub openai: Option<OpenAIConfig>,
    
    /// Google Calendar API の設定（オプション）
    pub google_calendar: Option<GoogleCalendarConfig>,
}

/// 一般設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// データ保存ディレクトリ
    pub data_dir: String,
    
    /// 自動登録の信頼度しきい値（0.0-1.0）
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,
    
    /// データ収集時間間隔（秒）
    #[serde(default = "default_collect_interval")]
    pub collect_interval_secs: u64,
    
    /// 1時間あたりの時間ブロック分割数（4=15分ごと、2=30分ごと、1=1時間ごと）
    #[serde(default = "default_time_block_division")]
    pub time_block_division: u8,
    
    #[serde(default = "default_idle_threshold")]
    pub idle_threshold_secs: u64,
}

/// Toggl API 設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TogglConfig {
    /// Toggl API トークン
    pub api_token: String,
    
    /// ワークスペースID
    pub workspace_id: u64,
}

/// OpenAI API 設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    /// OpenAI API キー
    pub api_key: String,
    
    /// 使用するモデル
    #[serde(default = "default_model")]
    pub model: String,
}

/// Google Calendar API 設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCalendarConfig {
    /// クライアントID
    pub client_id: String,
    
    /// クライアントシークレット
    pub client_secret: String,
    
    /// リフレッシュトークン
    pub refresh_token: String,
    
    /// カレンダーID（カンマ区切りで複数指定可能）
    pub calendar_ids: String,
}

// デフォルト値
fn default_confidence_threshold() -> f64 {
    0.5
}

fn default_collect_interval() -> u64 {
    60 // 1分
}

fn default_time_block_division() -> u8 {
    4 // 15分ごと
}

fn default_idle_threshold() -> u64 {
    300 // デフォルトは5分
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

/// 設定ファイルを読み込む
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<AppConfig> {
    let config_str = read_to_string(path)
        .context("Failed to read config file")?;
    
    let config: AppConfig = toml::from_str(&config_str)
        .context("Failed to parse config file")?;
    
    Ok(config)
}

/// デフォルトの設定を作成する
pub fn create_default_config() -> AppConfig {
    AppConfig {
        general: GeneralConfig {
            data_dir: "./data".to_string(),
            confidence_threshold: default_confidence_threshold(),
            collect_interval_secs: default_collect_interval(),
            time_block_division: default_time_block_division(),
            idle_threshold_secs: default_idle_threshold(),
        },
        toggl: TogglConfig {
            api_token: "your_toggl_api_token".to_string(),
            workspace_id: 0,
        },
        openai: Some(OpenAIConfig {
            api_key: "your_openai_api_key".to_string(),
            model: default_model(),
        }),
        google_calendar: None,
    }
}

/// サンプル設定ファイルを作成する
pub fn generate_sample_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let config = create_default_config();
    let toml_str = toml::to_string_pretty(&config)
        .context("Failed to serialize config")?;
    
    std::fs::write(path, toml_str)
        .context("Failed to write sample config file")?;
    
    Ok(())
} 
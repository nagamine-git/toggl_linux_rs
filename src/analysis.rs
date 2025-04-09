use anyhow::{Context, Result};
use async_openai::{
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequest,
        ChatCompletionResponseFormat, ChatCompletionResponseFormatType,
    },
    Client, config::OpenAIConfig,
};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

use crate::config::AppConfig;
use crate::data_collector::CollectedData;

/// 分析結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// 推定された活動内容
    pub activity: String,
    
    /// 推定の確度（0.0-1.0）
    pub confidence: f64,
    
    /// 推定に使用したデータのタイムスタンプ
    pub timestamp: chrono::DateTime<chrono::Utc>,
    
    /// 候補となる活動のリスト（確度が低い場合に使用）
    pub alternatives: Vec<ActivityCandidate>,
    
    /// 現在のウィンドウタイトル（タグ付け用）
    pub window_title: Option<String>,
    
    /// 関連するカレンダーイベント（タグ付け用）
    pub calendar_event: Option<crate::data_collector::CalendarEvent>,
}

/// 活動候補
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityCandidate {
    /// 活動内容
    pub activity: String,
    
    /// 確度
    pub confidence: f64,
}

/// GPT-4o miniを使って分析を実行
pub async fn analyze_with_gpt(
    config: &AppConfig,
    data: &[CollectedData],
) -> Result<AnalysisResult> {
    if data.is_empty() {
        return Err(anyhow::anyhow!("No data to analyze"));
    }
    
    if config.openai.is_none() {
        return Err(anyhow::anyhow!("OpenAI configuration is missing"));
    }
    
    let openai_config = config.openai.as_ref().unwrap();
    
    // APIキーを環境変数にセット
    env::set_var("OPENAI_API_KEY", &openai_config.api_key);
    
    // 分析用のプロンプトを構築
    let prompt = build_analysis_prompt(data);
    debug!("Analysis prompt: {}", prompt);
    
    // OpenAI クライアントの初期化
    let config = OpenAIConfig::new().with_api_key(openai_config.api_key.clone());
    let client = Client::with_config(config);
    
    // チャットメッセージを作成
    let messages = vec![
        ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessageArgs::default()
                .content("あなたはLinuxデスクトップ環境でのユーザーの活動を分析するAIアシスタントです。\
                ウィンドウタイトルやカレンダーイベントの情報から、ユーザーが何をしていたかを推定し、\
                その確度（0.0-1.0の値）を判断してください。\
                また、確度が低い場合は候補となる活動のリストも提供してください。")
                .build()?
        ),
        ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessageArgs::default()
                .content(prompt)
                .build()?
        ),
    ];
    
    // チャット完了リクエストを作成
    let response_format = ChatCompletionResponseFormat {
        r#type: ChatCompletionResponseFormatType::JsonObject,
    };
    
    let request = CreateChatCompletionRequest {
        model: openai_config.model.clone(),
        messages,
        temperature: Some(0.3),
        max_tokens: Some(500u16),
        response_format: Some(response_format),
        ..Default::default()
    };
    
    // APIリクエストを送信
    let response = client.chat().create(request).await?;
    
    if let Some(choice) = response.choices.first() {
        let content = choice.message.content.as_deref().unwrap_or("");
        debug!("GPT response: {}", content);
        
        // レスポンスをパースして分析結果を抽出
        parse_gpt_response(content, data)
    } else {
        Err(anyhow::anyhow!("No response from GPT"))
    }
}

/// ローカルな推論エンジンで分析を実行（オフライン時に使用）
pub fn analyze_locally(data: &[CollectedData]) -> Result<AnalysisResult> {
    if data.is_empty() {
        return Err(anyhow::anyhow!("No data to analyze"));
    }
    
    // 最新のデータポイントのタイムスタンプを使用
    let timestamp = data.first().unwrap().timestamp;
    
    // ウィンドウタイトルのカウントを集計
    let mut title_counts: HashMap<String, usize> = HashMap::new();
    
    for item in data {
        let title = item.window.title.to_lowercase();
        *title_counts.entry(title).or_insert(0) += 1;
    }
    
    // 最も頻度が高いタイトルを特定
    let mut most_frequent = ("".to_string(), 0);
    for (title, count) in &title_counts {
        if *count > most_frequent.1 {
            most_frequent = (title.clone(), *count);
        }
    }
    
    // 信頼度を計算（最も頻度が高いタイトルの占める割合）
    let confidence = most_frequent.1 as f64 / data.len() as f64;
    
    // 簡易的なキーワードマッチングでカテゴリを推定
    let activity = categorize_by_keywords(&most_frequent.0);
    
    // 候補リストを作成（上位3つまで）
    let mut alternatives = Vec::new();
    for (title, count) in title_counts.iter().filter(|(t, _)| *t != &most_frequent.0) {
        let conf = *count as f64 / data.len() as f64;
        alternatives.push(ActivityCandidate {
            activity: categorize_by_keywords(title),
            confidence: conf,
        });
        
        if alternatives.len() >= 3 {
            break;
        }
    }
    
    // 現在時刻に重なるカレンダーイベントを抽出
    let mut calendar_event = None;
    for item in data {
        for event in &item.calendar_events {
            if event.start_time <= timestamp && event.end_time >= timestamp {
                calendar_event = Some(event.clone());
                break;
            }
        }
        if calendar_event.is_some() {
            break;
        }
    }
    
    // 最も頻度が高いウィンドウタイトルから活動を推定
    Ok(AnalysisResult {
        activity,
        confidence,
        timestamp,
        alternatives,
        window_title: Some(most_frequent.0),
        calendar_event,
    })
}

/// キーワードベースで活動カテゴリを推定する簡易関数
fn categorize_by_keywords(title: &str) -> String {
    let title = title.to_lowercase();
    
    // キーワードマッチング（非常に簡易的な実装）
    if title.contains("firefox") || title.contains("chrome") || title.contains("edge") {
        if title.contains("gmail") || title.contains("mail") {
            return "メール確認".to_string();
        } else if title.contains("google doc") || title.contains("document") {
            return "ドキュメント作成".to_string();
        } else if title.contains("calendar") {
            return "スケジュール確認".to_string();
        } else if title.contains("youtube") || title.contains("video") {
            return "動画視聴".to_string();
        } else if title.contains("chat") || title.contains("slack") || title.contains("discord") {
            return "チャット/コミュニケーション".to_string();
        } else {
            return "ウェブブラウジング".to_string();
        }
    } else if title.contains("terminal") || title.contains("console") || title.contains("bash") {
        return "ターミナル作業".to_string();
    } else if title.contains("code") || title.contains("vscode") || title.contains("intellij") {
        return "プログラミング".to_string();
    } else if title.contains("libreoffice") || title.contains("calc") || title.contains("writer") {
        return "オフィス作業".to_string();
    } else if title.contains("gimp") || title.contains("photoshop") || title.contains("illustrator") {
        return "画像編集".to_string();
    } else if title.contains("meeting") || title.contains("zoom") || title.contains("teams") {
        return "ミーティング".to_string();
    }
    
    // デフォルト
    "その他の活動".to_string()
}

/// 分析用のプロンプトを構築
fn build_analysis_prompt(data: &[CollectedData]) -> String {
    let mut prompt = String::from(
        "以下のLinuxデスクトップのウィンドウ情報とカレンダーイベントから、ユーザーの活動内容を推定し、その確度（0.0-1.0）を評価してください。\n\n"
    );
    
    // データ形式を説明
    prompt.push_str("### ウィンドウ情報 ###\n");
    prompt.push_str("タイムスタンプ | ウィンドウタイトル | クラス\n");
    
    // ウィンドウ情報を追加
    for item in data {
        prompt.push_str(&format!(
            "{} | {} | {}\n",
            item.timestamp.format("%Y-%m-%d %H:%M:%S"),
            item.window.title,
            item.window.class.as_deref().unwrap_or("不明")
        ));
    }
    
    // カレンダーイベント情報があれば追加
    // 現在時刻にかぶっているイベントだけをフィルタリングして重複を除く
    let has_calendar_events = data.iter().any(|d| !d.calendar_events.is_empty());
    if has_calendar_events {
        prompt.push_str("\n### カレンダーイベント ###\n");
        prompt.push_str("タイトル | 開始時間 | 終了時間\n");
        
        // 重複を避けるためにイベントIDをキーとするマップを使用
        let mut seen_events = std::collections::HashSet::new();
        
        // 現在時刻
        let now = if !data.is_empty() {
            data[0].timestamp
        } else {
            chrono::Utc::now()
        };
        
        for item in data {
            for event in &item.calendar_events {
                // イベントIDでの重複チェック
                if seen_events.contains(&event.id) {
                    continue;
                }
                
                // 現在時刻にかぶっているイベントのみを含める
                if event.start_time <= now && event.end_time >= now {
                    prompt.push_str(&format!(
                        "{} | {} | {}\n",
                        event.title,
                        event.start_time.format("%Y-%m-%d %H:%M:%S"),
                        event.end_time.format("%Y-%m-%d %H:%M:%S")
                    ));
                    
                    seen_events.insert(event.id.clone());
                }
            }
        }
    }
    
    // 出力形式の指定
    prompt.push_str("\nこの情報を元に、以下の形式でJSON形式で回答してください：\n");
    prompt.push_str("{\n");
    prompt.push_str("  \"activity\": \"推定される活動内容\",\n");
    prompt.push_str("  \"confidence\": 0.0～1.0の値,\n");
    prompt.push_str("  \"alternatives\": [\n");
    prompt.push_str("    { \"activity\": \"候補1\", \"confidence\": 0.0～1.0の値 },\n");
    prompt.push_str("    { \"activity\": \"候補2\", \"confidence\": 0.0～1.0の値 }\n");
    prompt.push_str("  ]\n");
    prompt.push_str("}\n");
    
    prompt
}

/// GPTのレスポンスをパースして分析結果を抽出
fn parse_gpt_response(
    response: &str,
    data: &[CollectedData],
) -> Result<AnalysisResult> {
    if data.is_empty() {
        return Err(anyhow::anyhow!("No data available for analysis"));
    }
    
    // 最新のデータポイントのタイムスタンプとウィンドウタイトルを使用
    let timestamp = data.first().unwrap().timestamp;
    let window_title = data.first().map(|d| d.window.title.clone());
    
    // 現在時刻に重なるカレンダーイベントを抽出
    let mut calendar_event = None;
    for item in data {
        for event in &item.calendar_events {
            if event.start_time <= timestamp && event.end_time >= timestamp {
                calendar_event = Some(event.clone());
                break;
            }
        }
        if calendar_event.is_some() {
            break;
        }
    }
    
    // JSONレスポンスをパース
    let parsed: serde_json::Value = serde_json::from_str(response)
        .context("Failed to parse GPT response as JSON")?;
    
    // 活動内容を抽出
    let activity = parsed["activity"].as_str()
        .ok_or_else(|| anyhow::anyhow!("No activity in response"))?
        .to_string();
    
    // 確度を抽出
    let confidence = parsed["confidence"].as_f64()
        .ok_or_else(|| anyhow::anyhow!("No confidence in response"))?;
    
    // 候補リストを抽出
    let mut alternatives = Vec::new();
    if let Some(alts) = parsed["alternatives"].as_array() {
        for alt in alts {
            if let (Some(alt_activity), Some(alt_confidence)) = (
                alt["activity"].as_str(),
                alt["confidence"].as_f64(),
            ) {
                alternatives.push(ActivityCandidate {
                    activity: alt_activity.to_string(),
                    confidence: alt_confidence,
                });
            }
        }
    }
    
    Ok(AnalysisResult {
        activity,
        confidence,
        timestamp,
        alternatives,
        window_title,
        calendar_event,
    })
} 
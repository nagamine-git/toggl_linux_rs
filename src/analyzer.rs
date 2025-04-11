use std::error::Error;
use log::info;
use crate::data_collector::CollectedData;
use crate::toggl::TogglClient;

const CONFIDENCE_THRESHOLD: f64 = 0.7;

pub async fn analyze_and_register(data: &CollectedData, toggl: &TogglClient) -> Result<(), Box<dyn Error>> {
    info!("Running analysis on collected data");

    // アイドル状態の場合は登録をスキップ
    if data.is_idle {
        info!("System is idle, skipping analysis and registration");
        return Ok(());
    }

    // 分析を実行
    let analysis_result = analyze_data(data)?;
    info!("Analysis result: activity='{}', confidence={}", analysis_result.activity, analysis_result.confidence);

    // 信頼度が閾値以上の場合のみ自動登録
    if analysis_result.confidence >= CONFIDENCE_THRESHOLD {
        info!("Confidence above threshold, auto-registering");
        toggl.create_time_entry(&TimeEntry {
            description: analysis_result.activity,
            wid: toggl.workspace_id,
            start: data.timestamp.to_rfc3339(),
            duration: None,
            created_with: Some("toggl_linux_rs".to_string()),
            ..Default::default()
        }).await?;
    } else {
        info!("Confidence below threshold, skipping registration");
    }

    Ok(())
}

#[derive(Debug)]
struct AnalysisResult {
    activity: String,
    confidence: f64,
}

/// データを分析して活動を推測する
fn analyze_data(data: &CollectedData) -> Result<AnalysisResult, Box<dyn Error>> {
    // ウィンドウタイトルとカレンダーイベントから活動を推測
    let activity = if !data.calendar_events.is_empty() {
        // カレンダーイベントがある場合は、最初のイベントのタイトルを使用
        data.calendar_events[0].title.clone()
    } else {
        // ウィンドウタイトルから活動を推測
        data.window.title.clone()
    };

    Ok(AnalysisResult {
        activity,
        confidence: 0.9, // 仮の信頼度
    })
} 
use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time;
use chrono::{self, Local, Timelike};
use std::io::Write;

mod config;
mod data_collector;
mod analysis;
mod event;
mod utils;
mod wizard;

use config::AppConfig;
use wizard::ConfigWizard;

/// Linux automatic activity tracking with Toggl integration
#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Args {
    /// Path to config file
    #[clap(short, long, value_parser, default_value = "config.toml")]
    config: PathBuf,

    /// Run in daemon mode
    #[clap(short, long)]
    daemon: bool,

    /// Analyze existing logs without collecting new data
    #[clap(long)]
    analyze_only: bool,
    
    /// Run configuration wizard
    #[clap(long)]
    wizard: bool,
    
    /// Add to XFCE autostart
    #[clap(long)]
    add_to_autostart: bool,
}

/// アプリケーションのロギングを初期化
fn init_logging() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] {}: {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
    
    info!("toggl_linux_rs v{} を起動しました", env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初期化処理をカスタム関数に置き換え
    init_logging()?;
    
    let args = Args::parse();
    
    // XFCE自動起動に追加
    if args.add_to_autostart {
        info!("Adding application to XFCE autostart");
        return utils::add_to_xfce_autostart();
    }
    
    // 設定ウィザードを実行
    if args.wizard {
        info!("Starting configuration wizard");
        let wizard = ConfigWizard::new();
        return wizard.run().await;
    }
    
    // 設定ファイルを読み込む
    let config = config::load_config(&args.config)
        .context("Failed to load configuration")?;
    
    info!("Starting toggl_linux_rs v{}", env!("CARGO_PKG_VERSION"));
    
    if args.analyze_only {
        // 過去のログファイルを分析するモード
        info!("Running in analyze-only mode");
        analyze_logs(&config).await?;
        return Ok(());
    }
    
    if args.daemon {
        // デーモンモードで実行
        info!("Running in daemon mode");
        run_daemon(&config).await?;
    } else {
        // 一回だけ実行するモード
        info!("Running in one-shot mode");
        run_once(&config).await?;
    }
    
    Ok(())
}

/// データ収集と分析を一度だけ実行する
async fn run_once(config: &AppConfig) -> Result<()> {
    // アクティブウィンドウ情報を取得
    let window_info = data_collector::get_active_window()
        .context("Failed to get active window information")?;
    
    info!("Current window: {}", window_info.title);
    
    // カレンダー情報があれば取得
    if let Some(calendar_config) = &config.google_calendar {
        match data_collector::get_calendar_events(calendar_config).await {
            Ok(events) => {
                info!("Retrieved {} calendar events", events.len());
            }
            Err(e) => {
                error!("Failed to get calendar events: {}", e);
            }
        }
    }
    
    Ok(())
}

/// デーモンモードでデータ収集と分析を定期的に実行する
async fn run_daemon(config: &AppConfig) -> Result<()> {
    // 収集データの保存先を初期化
    data_collector::init_storage().context("Failed to initialize storage")?;
    
    // データコレクターを初期化
    let mut collector = data_collector::DataCollector::new(config.clone())
        .context("Failed to initialize data collector")?;
    
    // インターネット接続を確認
    if !utils::check_internet_connection() {
        utils::send_notification(
            "toggl_linux_rs",
            "インターネット接続がありません。一部の機能が制限される場合があります。",
            Some("warning")
        )?;
        info!("No internet connection detected. Some features may be limited.");
    }
    
    // 1分ごとのデータ収集タイマー
    let collect_interval = Duration::from_secs(config.general.collect_interval_secs);
    
    // 時間ブロックの分割設定を取得（デフォルト：4=15分ごと）
    let time_block_division = config.general.time_block_division;
    let minutes_per_block = 60 / time_block_division as u64;
    
    info!("Using time block division: {} blocks per hour ({} minutes per block)", 
          time_block_division, minutes_per_block);
    
    // 次のタイムブロック終了時刻までの時間を計算
    let now = chrono::Utc::now();
    let current_minute = now.minute() % 60;
    let minutes_to_next_block = minutes_per_block - (current_minute as u64 % minutes_per_block);
    let seconds_to_next_block = minutes_to_next_block * 60 - now.second() as u64;
    
    // 次のタイムブロック境界までの待機時間を設定
    let initial_delay = Duration::from_secs(seconds_to_next_block);
    info!("Scheduling first analysis in {} minutes and {} seconds", 
          minutes_to_next_block, now.second());
    
    // 時間ブロックごとの分析タイマー
    let analysis_interval = Duration::from_secs(minutes_per_block * 60);
    let mut analysis_timer = time::interval_at(
        time::Instant::now() + initial_delay,
        analysis_interval
    );
    
    // メインループ
    let mut collect_timer = time::interval(collect_interval);
    let mut collected_data_count = 0;
    
    loop {
        tokio::select! {
            // データ収集ループ
            _ = collect_timer.tick() => {
                match collector.collect().await {
                    Ok(_) => {
                        collected_data_count += 1;
                        info!("Collected data point #{}", collected_data_count);
                    }
                    Err(e) => {
                        error!("Error collecting data: {}", e);
                    }
                }
            }
            
            // 分析ループ (タイムブロック境界ごとに実行)
            _ = analysis_timer.tick() => {
                let now = chrono::Utc::now();
                info!("Running analysis at time block: {:02}:{:02}", now.hour(), now.minute());
                
                if collected_data_count > 0 {
                    // インターネット接続を確認
                    if !utils::check_internet_connection() {
                        info!("Skipping analysis due to no internet connection");
                        continue;
                    }
                    
                    info!("Running analysis on collected data");
                    if let Err(e) = analyze_and_register(config).await {
                        error!("Error during analysis: {}", e);
                    }
                }
            }
        }
    }
}

/// 保存されたログファイルを分析する
async fn analyze_logs(config: &AppConfig) -> Result<()> {
    info!("Analyzing saved logs");
    analyze_and_register(config).await
}

/// データを分析し、条件に応じてTogglに登録する
async fn analyze_and_register(config: &AppConfig) -> Result<()> {
    // 最近のデータを取得
    let recent_data = data_collector::get_recent_data()?;
    
    if recent_data.is_empty() {
        info!("No recent data to analyze");
        return Ok(());
    }
    
    // 分析を実行
    let analysis_result = match config.openai.is_some() {
        true => {
            info!("Using GPT-4o mini for analysis");
            analysis::analyze_with_gpt(config, &recent_data).await?
        }
        false => {
            info!("Using local analysis engine");
            analysis::analyze_locally(&recent_data)?
        }
    };
    
    info!(
        "Analysis result: activity='{}', confidence={}",
        analysis_result.activity, analysis_result.confidence
    );
    
    // 分析結果に基づいて登録処理
    if analysis_result.confidence >= 0.5 {
        info!("Confidence above threshold, auto-registering");
        match event::register_to_toggl(config, &analysis_result).await {
            Ok(_) => {
                info!("Successfully registered to Toggl");
            }
            Err(e) => {
                error!("Failed to register to Toggl: {}", e);
            }
        }
    } else {
        info!("Confidence below threshold, user confirmation required");
        // ここで通知や対話的な確認を行う予定
    }
    
    Ok(())
}

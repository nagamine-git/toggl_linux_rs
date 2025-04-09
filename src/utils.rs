use anyhow::{Context, Result};
use log::info;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// ユーザーホームディレクトリのパスを取得
pub fn get_home_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to determine home directory"))
}

/// XDGデータディレクトリを取得
pub fn get_data_dir() -> Result<PathBuf> {
    let data_dir = if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(dir)
    } else {
        get_home_dir()?.join(".local").join("share")
    };
    
    let app_data_dir = data_dir.join("toggl_linux_rs");
    
    if !app_data_dir.exists() {
        fs::create_dir_all(&app_data_dir)
            .context("Failed to create data directory")?;
    }
    
    Ok(app_data_dir)
}

/// XDG設定ディレクトリを取得
pub fn get_config_dir() -> Result<PathBuf> {
    let config_dir = if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(dir)
    } else {
        get_home_dir()?.join(".config")
    };
    
    let app_config_dir = config_dir.join("toggl_linux_rs");
    
    if !app_config_dir.exists() {
        fs::create_dir_all(&app_config_dir)
            .context("Failed to create config directory")?;
    }
    
    Ok(app_config_dir)
}

/// デスクトップ通知を送信
pub fn send_notification(
    title: &str,
    message: &str,
    urgency: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("notify-send");
    
    cmd.arg("--app-name=toggl_linux_rs")
        .arg(title)
        .arg(message);
    
    if let Some(u) = urgency {
        cmd.arg("--urgency").arg(u);
    }
    
    let output = cmd.output()
        .context("Failed to execute notify-send")?;
    
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to send notification: {}", err));
    }
    
    Ok(())
}

/// ネット接続状態をチェック
pub fn check_internet_connection() -> bool {
    // Googleのパブリックなサーバーに接続してネット状態をチェック
    let output = Command::new("ping")
        .args(["-c", "1", "-W", "1", "8.8.8.8"])
        .output();
    
    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// アプリケーションをXfceの自動起動に追加
pub fn add_to_xfce_autostart() -> Result<()> {
    let autostart_dir = get_home_dir()?.join(".config").join("autostart");
    
    if !autostart_dir.exists() {
        fs::create_dir_all(&autostart_dir)
            .context("Failed to create autostart directory")?;
    }
    
    let desktop_file_path = autostart_dir.join("toggl_linux_rs.desktop");
    
    // 実行ファイルのパスを取得
    let executable_path = std::env::current_exe()
        .context("Failed to get executable path")?;
    
    let content = format!(
        r#"[Desktop Entry]
Type=Application
Name=toggl_linux_rs
Comment=Automatic activity tracking for Linux with Toggl integration
Exec={} --daemon
Icon=toggl
Terminal=false
Categories=Utility;
StartupNotify=false
"#,
        executable_path.display()
    );
    
    let mut file = File::create(&desktop_file_path)
        .context("Failed to create desktop file")?;
    
    file.write_all(content.as_bytes())
        .context("Failed to write desktop file")?;
    
    info!("Added to Xfce autostart at {:?}", desktop_file_path);
    
    Ok(())
} 
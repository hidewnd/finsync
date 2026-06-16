use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{self, ConnectionConfig};

const LOG_FILE_NAME: &str = "finsync.log";

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init_from_config_path(config_path: &Path) -> Result<PathBuf, String> {
    let log_dir = resolve_log_dir(config_path)?;
    fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    let _ = LOG_DIR.set(log_dir.clone());
    append_log_to_dir(&log_dir, "INFO", "日志初始化完成")?;
    Ok(log_dir)
}

pub fn resolve_log_dir(config_path: &Path) -> Result<PathBuf, String> {
    let app_config = config::parse_app_config(&config_path.to_string_lossy())?;
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let configured = app_config.log_path.unwrap_or_else(|| PathBuf::from("logs"));
    if configured.is_absolute() {
        Ok(configured)
    } else {
        Ok(config_dir.join(configured))
    }
}

pub fn append_log_to_dir(log_dir: &Path, level: &str, message: &str) -> Result<(), String> {
    fs::create_dir_all(log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    let path = log_dir.join(LOG_FILE_NAME);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("打开日志文件失败: {}", e))?;

    writeln!(file, "{} [{}] {}", timestamp(), level, message)
        .map_err(|e| format!("写入日志失败: {}", e))
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn debug(message: impl AsRef<str>) {
    write("DEBUG", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

pub fn format_connection_for_log(cfg: &ConnectionConfig) -> String {
    format!(
        "name={} host={} port={} user={}",
        cfg.name, cfg.host, cfg.port, cfg.user
    )
}

fn write(level: &str, message: &str) {
    let Some(log_dir) = LOG_DIR.get() else {
        return;
    };

    if let Err(err) = append_log_to_dir(log_dir, level, message) {
        eprintln!("写入日志失败: {}", err);
    }
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix={}", secs)
}

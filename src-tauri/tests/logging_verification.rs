use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_temp_dir() -> TempDirGuard {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "finsync_logging_test_{}_{}",
        std::process::id(),
        id
    ));
    fs::create_dir_all(&path).expect("create temp dir");
    TempDirGuard(path)
}

fn write_config(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("config.ini");
    let mut file = fs::File::create(&path).expect("create config");
    file.write_all(content.as_bytes()).expect("write config");
    path
}

#[test]
fn resolve_log_dir_defaults_to_config_sibling_logs() {
    let temp = make_temp_dir();
    let config_path = write_config(&temp.0, "[local]\nhost=127.0.0.1\n");

    let log_dir = app_lib::logging::resolve_log_dir(&config_path).expect("resolve log dir");

    assert_eq!(log_dir, temp.0.join("logs"));
}

#[test]
fn resolve_log_dir_uses_relative_path_from_config_dir() {
    let temp = make_temp_dir();
    let config_path = write_config(&temp.0, "log_path=custom/logs\n[local]\nhost=127.0.0.1\n");

    let log_dir = app_lib::logging::resolve_log_dir(&config_path).expect("resolve log dir");

    assert_eq!(log_dir, temp.0.join("custom").join("logs"));
}

#[test]
fn resolve_log_dir_uses_absolute_path_as_is() {
    let temp = make_temp_dir();
    let absolute_log_dir = temp.0.join("absolute-logs");
    let config_path = write_config(
        &temp.0,
        &format!(
            "log_path={}\n[local]\nhost=127.0.0.1\n",
            absolute_log_dir.to_string_lossy()
        ),
    );

    let log_dir = app_lib::logging::resolve_log_dir(&config_path).expect("resolve log dir");

    assert_eq!(log_dir, absolute_log_dir);
}

#[test]
fn append_log_creates_directory_and_writes_utf8_lines() {
    let temp = make_temp_dir();
    let log_dir = temp.0.join("nested").join("logs");

    app_lib::logging::append_log_to_dir(&log_dir, "INFO", "启动完成：中文日志")
        .expect("write first log");
    app_lib::logging::append_log_to_dir(&log_dir, "DEBUG", "SELECT 1").expect("write second log");

    let log_path = log_dir.join("finsync.log");
    let content = fs::read_to_string(&log_path).expect("read log file");
    assert!(content.contains("[INFO] 启动完成：中文日志"));
    assert!(content.contains("[DEBUG] SELECT 1"));
}

#[test]
fn format_connection_for_log_excludes_password() {
    let cfg = app_lib::config::ConnectionConfig {
        name: "prod".to_string(),
        host: "127.0.0.1".to_string(),
        port: "3306".to_string(),
        user: "root".to_string(),
        password: "secret-password".to_string(),
    };

    let line = app_lib::logging::format_connection_for_log(&cfg);

    assert!(line.contains("name=prod"));
    assert!(line.contains("host=127.0.0.1"));
    assert!(line.contains("port=3306"));
    assert!(line.contains("user=root"));
    assert!(!line.contains("secret-password"));
    assert!(!line.contains("password"));
}

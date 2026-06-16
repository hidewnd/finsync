use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Top-level application settings from config.ini.
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub log_path: Option<PathBuf>,
}

/// Represents a single MySQL connection configuration.
/// Mirrors the Go `ConnectionConfig` struct from config.go.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConnectionConfig {
    pub name: String,
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
}

/// Finds the config.ini file path.
///
/// Search order (mirrors Go `FindConfigPath`):
/// 1. Executable directory (for production)
/// 2. Current working directory (for development)
/// 3. Executable directory without checking existence (last resort)
pub fn find_config_path() -> String {
    // Try exe directory first (for production use)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let cfg_path = exe_dir.join("config.ini");
            if cfg_path.exists() {
                return cfg_path.to_string_lossy().to_string();
            }
        }
    }

    // Fallback to working directory (for development)
    let cwd_cfg = Path::new("config.ini");
    if cwd_cfg.exists() {
        if let Ok(abs) = cwd_cfg.canonicalize() {
            return abs.to_string_lossy().to_string();
        }
    }

    // Last resort: exe directory without file existing
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    exe_dir.join("config.ini").to_string_lossy().to_string()
}

/// Parses top-level application settings before the first connection section.
pub fn parse_app_config(path: &str) -> Result<AppConfig, String> {
    let file = fs::File::open(path).map_err(|e| format!("无法打开配置文件: {}", e))?;
    let reader = BufReader::new(file);
    let mut app_config = AppConfig { log_path: None };

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("读取配置文件失败: {}", e))?;
        let line = line.trim().to_string();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            break;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        if key.trim().eq_ignore_ascii_case("log_path") {
            let value = value.trim();
            if !value.is_empty() {
                app_config.log_path = Some(PathBuf::from(value));
            }
        }
    }

    Ok(app_config)
}

/// Parses an INI-style config file and returns connection configurations.
///
/// Supports two formats:
///
/// **Format 1 — Individual fields:**
/// ```ini
/// [connection_name]
/// host=xxx
/// port=3306
/// user=root
/// password=xxx
/// ```
///
/// **Format 2 — JDBC-style URL:**
/// ```ini
/// [connection_name]
/// url=jdbc:mysql://host:port/database?param1=val1&param2=val2
/// user=root
/// password=xxx
/// ```
///
/// If both `url` and `host`/`port` are present in the same section,
/// `url` takes precedence (matching the Go config.go behavior).
pub fn parse_config(path: &str) -> Result<Vec<ConnectionConfig>, String> {
    let file = fs::File::open(path).map_err(|e| format!("无法打开配置文件: {}", e))?;
    let reader = BufReader::new(file);

    let mut configs: Vec<ConnectionConfig> = Vec::new();
    let mut current: Option<ConnectionConfig> = None;
    // JDBC URL value deferred so it always takes precedence over host/port
    // regardless of key ordering within the section.
    let mut pending_url: Option<String> = None;

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("读取配置文件失败: {}", e))?;
        let line = line.trim().to_string();

        // Skip blank lines and comments (; or #)
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        // Section header: [name]
        if line.starts_with('[') && line.ends_with(']') {
            // Save previous section — apply pending URL before saving
            if let Some(ref mut cfg) = current {
                if let Some(url) = pending_url.take() {
                    parse_jdbc_url(cfg, &url);
                }
                configs.push(cfg.clone());
            }
            let name = line[1..line.len() - 1].to_string();
            current = Some(ConnectionConfig {
                name,
                host: String::new(),
                port: String::new(),
                user: String::new(),
                password: String::new(),
            });
            pending_url = None;
            continue;
        }

        // Skip lines without a current section
        let Some(ref mut cfg) = current else {
            continue;
        };

        // Parse key=value
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim().to_lowercase();
        let value = value.trim().to_string();

        match key.as_str() {
            "host" => {
                cfg.host = value;
            }
            "port" => {
                cfg.port = value;
            }
            "url" => {
                // Defer URL parsing: JDBC URL always takes precedence over host/port,
                // so we apply it after all host/port keys have been processed.
                pending_url = Some(value);
            }
            "user" => {
                cfg.user = value;
            }
            "password" => {
                cfg.password = value;
            }
            _ => {
                // Unknown keys are ignored (forward-compatible)
            }
        }
    }

    // Save the last section — apply pending URL before saving
    if let Some(ref mut cfg) = current {
        if let Some(url) = pending_url.take() {
            parse_jdbc_url(cfg, &url);
        }
        configs.push(cfg.clone());
    }

    Ok(configs)
}

/// Parses a JDBC-style MySQL URL and sets Host and Port on the config.
///
/// Accepts formats:
/// - `jdbc:mysql://host:port/`
/// - `jdbc:mysql://host:port/database?params`
/// - `jdbc:mysql://host/database?params`  (port defaults to "3306")
/// - `jdbc:mysql://host:port/db?useSSL=false&charset=utf8`
///
/// Mirrors the Go `parseJDBCURL` function from config.go.
fn parse_jdbc_url(cfg: &mut ConnectionConfig, jdbc_url: &str) {
    let u = jdbc_url.trim();

    // Strip "jdbc:" prefix
    let u = if let Some(rest) = u.strip_prefix("jdbc:") {
        rest
    } else if u.starts_with("mysql://") {
        u
    } else {
        return;
    };

    // Must start with "mysql://"
    let u = if let Some(rest) = u.strip_prefix("mysql://") {
        rest
    } else {
        return;
    };

    // Split on '/' to get the host:port portion (before any path)
    let host_port = u.split('/').next().unwrap_or("");

    if host_port.is_empty() {
        return;
    }

    // Split host and port
    if let Some((host, port_str)) = host_port.split_once(':') {
        cfg.host = host.to_string();
        cfg.port = port_str.to_string();
    } else {
        cfg.host = host_port.to_string();
        cfg.port = "3306".to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Guard that removes a temp file on drop (panic-safe cleanup).
    struct TempFileGuard(PathBuf);

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    /// Helper: create a temp config file with the given content and return a guard.
    /// The file is automatically removed when the guard goes out of scope.
    fn write_temp_config(content: &str) -> TempFileGuard {
        let dir = std::env::temp_dir();
        let id = FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = dir.join(format!("test_config_{}_{}.ini", std::process::id(), id));
        let mut f = fs::File::create(&path).expect("failed to create temp file");
        f.write_all(content.as_bytes())
            .expect("failed to write temp file");
        TempFileGuard(path)
    }

    // ── c1: Traditional format (host/port/user/password) ──

    #[test]
    fn test_parse_traditional_format() {
        let content = r#"[本地开发库]
host=127.0.0.1
port=3306
user=root
password=root
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "本地开发库");
        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "root");
        assert_eq!(configs[0].password, "root");
    }

    #[test]
    fn test_parse_multiple_sections_traditional() {
        let content = r#"[section1]
host=10.0.0.1
port=3306
user=admin
password=pass1

[section2]
host=10.0.0.2
port=3307
user=admin2
password=pass2
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "section1");
        assert_eq!(configs[0].host, "10.0.0.1");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[1].name, "section2");
        assert_eq!(configs[1].host, "10.0.0.2");
        assert_eq!(configs[1].port, "3307");
    }

    // ── c2: JDBC URL format ──

    #[test]
    fn test_parse_jdbc_url_with_port() {
        let content = r#"[jdbc-test]
url=jdbc:mysql://rm-bp1xxxxx.mysql.rds.aliyuncs.com:3306/?characterEncoding=utf8&useSSL=false
user=admin
password=secret
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "jdbc-test");
        assert_eq!(configs[0].host, "rm-bp1xxxxx.mysql.rds.aliyuncs.com");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "admin");
        assert_eq!(configs[0].password, "secret");
    }

    #[test]
    fn test_parse_jdbc_url_default_port() {
        let content = r#"[jdbc-no-port]
url=jdbc:mysql://db.example.com/somedb?charset=utf8
user=user1
password=pass1
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "db.example.com");
        assert_eq!(configs[0].port, "3306"); // default port
    }

    #[test]
    fn test_parse_jdbc_url_no_path() {
        let content = r#"[jdbc-no-path]
url=jdbc:mysql://127.0.0.1:3307/
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[0].port, "3307");
    }

    #[test]
    fn test_parse_jdbc_url_no_port_no_path() {
        let content = r#"[jdbc-simple]
url=jdbc:mysql://localhost
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "localhost");
        assert_eq!(configs[0].port, "3306");
    }

    #[test]
    fn test_parse_jdbc_url_invalid_format_no_panic() {
        // Invalid JDBC URLs should not panic, just leave host/port unchanged
        let content = r#"[bad-url]
url=not-a-valid-url
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "");
        assert_eq!(configs[0].port, "");
    }

    // ── c3: JDBC URL precedence over host/port ──

    #[test]
    fn test_jdbc_url_precedence_over_host_port() {
        // URL should win even if host/port appear after it in the section
        let content = r#"[precedence]
host=should-be-overridden
port=9999
url=jdbc:mysql://correct.host:3306/db
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "correct.host");
        assert_eq!(configs[0].port, "3306");
    }

    #[test]
    fn test_jdbc_url_precedence_url_before_host() {
        // URL appears before host/port in the config — URL still wins
        let content = r#"[precedence2]
url=jdbc:mysql://url.wins:3307/db
host=should-lose
port=1111
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "url.wins");
        assert_eq!(configs[0].port, "3307");
    }

    // ── c4: Config file discovery ──

    #[test]
    fn test_find_config_path_returns_something() {
        // Basic sanity: find_config_path should return a path ending in config.ini
        let path = find_config_path();
        assert!(
            path.ends_with("config.ini"),
            "Expected path ending with config.ini, got: {}",
            path
        );
    }

    // ── c5: Comments and blank lines ignored ──

    #[test]
    fn test_comments_ignored() {
        let content = r#"; This is a comment
# This is also a comment
[commented-section]
; comment in the middle
host=10.0.0.1
# another comment
port=3306
user=admin

password=secret
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "commented-section");
        assert_eq!(configs[0].host, "10.0.0.1");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "admin");
        assert_eq!(configs[0].password, "secret");
    }

    #[test]
    fn test_blank_lines_ignored() {
        let content = r#"

[section-a]
host=1.1.1.1

port=3306
user=a
password=a

[section-b]

host=2.2.2.2
port=3307

user=b
password=b

"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "section-a");
        assert_eq!(configs[1].name, "section-b");
    }

    // ── c6: Comprehensive scenarios ──

    #[test]
    fn test_empty_file() {
        let content = "";
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");
        assert!(configs.is_empty());
    }

    #[test]
    fn test_only_comments() {
        let content = "; just a comment\n# another comment\n; and one more";
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");
        assert!(configs.is_empty());
    }

    #[test]
    fn test_mixed_traditional_and_jdbc() {
        let content = r#"[traditional]
host=192.168.1.100
port=3306
user=test_user
password=test_pass

[jdbc-section]
url=jdbc:mysql://rm-bp1xxxxx.mysql.rds.aliyuncs.com:3306/?characterEncoding=utf8&zeroDateTimeBehavior=convertToNull&useSSL=false
user=admin
password=your_password_here
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 2);

        // Traditional section
        assert_eq!(configs[0].name, "traditional");
        assert_eq!(configs[0].host, "192.168.1.100");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "test_user");

        // JDBC section
        assert_eq!(configs[1].name, "jdbc-section");
        assert_eq!(configs[1].host, "rm-bp1xxxxx.mysql.rds.aliyuncs.com");
        assert_eq!(configs[1].port, "3306");
        assert_eq!(configs[1].user, "admin");
    }

    #[test]
    fn test_missing_fields_default_to_empty() {
        let content = r#"[minimal]
host=127.0.0.1
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[0].port, "");
        assert_eq!(configs[0].user, "");
        assert_eq!(configs[0].password, "");
    }

    #[test]
    fn test_keys_before_section_ignored() {
        let content = r#"host=orphan-value
port=9999

[valid-section]
host=10.0.0.1
user=admin
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        // Only the section should be parsed; orphan key-values before any section are ignored
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "valid-section");
        assert_eq!(configs[0].host, "10.0.0.1");
    }

    #[test]
    fn test_parse_app_config_reads_top_level_log_path() {
        let content = r#"; app settings
log_path=logs/debug

[valid-section]
host=10.0.0.1
user=admin
"#;
        let guard = write_temp_config(content);
        let app_config = parse_app_config(&guard.0.to_string_lossy()).expect("parse app config");

        assert_eq!(app_config.log_path, Some(PathBuf::from("logs/debug")));
    }

    #[test]
    fn test_parse_app_config_ignores_section_log_path() {
        let content = r#"[valid-section]
log_path=should-not-be-app-setting
host=10.0.0.1
"#;
        let guard = write_temp_config(content);
        let app_config = parse_app_config(&guard.0.to_string_lossy()).expect("parse app config");

        assert_eq!(app_config.log_path, None);
    }

    #[test]
    fn test_parse_config_ignores_top_level_log_path() {
        let content = r#"log_path=logs

[valid-section]
host=10.0.0.1
user=admin
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse config");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "valid-section");
        assert_eq!(configs[0].host, "10.0.0.1");
    }

    #[test]
    fn test_real_config_file() {
        // Parse the actual config.ini from the Go project
        let config_path = Path::new("../../finsync/config.ini");
        if !config_path.exists() {
            eprintln!(
                "Skipping test: real config.ini not found at {:?}",
                config_path
            );
            return;
        }

        let configs = parse_config(&config_path.to_string_lossy()).expect("parse should succeed");
        assert_eq!(configs.len(), 3);

        // [本地开发库] - traditional format
        assert_eq!(configs[0].name, "本地开发库");
        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "root");
        assert_eq!(configs[0].password, "root");

        // [阿里云RDS_生产] - JDBC URL format
        assert_eq!(configs[1].name, "阿里云RDS_生产");
        assert_eq!(configs[1].host, "rm-bp1xxxxx.mysql.rds.aliyuncs.com");
        assert_eq!(configs[1].port, "3306");
        assert_eq!(configs[1].user, "admin");
        assert_eq!(configs[1].password, "your_password_here");

        // [测试环境] - traditional format
        assert_eq!(configs[2].name, "测试环境");
        assert_eq!(configs[2].host, "192.168.1.100");
        assert_eq!(configs[2].port, "3306");
        assert_eq!(configs[2].user, "test_user");
        assert_eq!(configs[2].password, "test_pass");
    }

    #[test]
    fn test_key_case_insensitive() {
        let content = r#"[case-test]
HOST=10.0.0.1
Port=3306
USER=Admin
PASSWORD=Secret
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "10.0.0.1");
        assert_eq!(configs[0].port, "3306");
        assert_eq!(configs[0].user, "Admin");
        assert_eq!(configs[0].password, "Secret");
    }

    #[test]
    fn test_file_not_found_error() {
        let result = parse_config("/nonexistent/path/config.ini");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("无法打开配置文件"),
            "Expected '无法打开配置文件' in error, got: {}",
            err
        );
    }

    #[test]
    fn test_section_with_no_fields() {
        let content = r#"[empty-section]
[section-with-fields]
host=10.0.0.1
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "empty-section");
        assert_eq!(configs[0].host, "");
        assert_eq!(configs[1].name, "section-with-fields");
        assert_eq!(configs[1].host, "10.0.0.1");
    }

    #[test]
    fn test_url_with_database_path() {
        let content = r#"[with-db]
url=jdbc:mysql://db.host:3307/mydatabase?charset=utf8
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "db.host");
        assert_eq!(configs[0].port, "3307");
    }

    #[test]
    fn test_jdbc_mysql_bare_prefix() {
        // Test bare mysql:// URL without jdbc: prefix
        let content = r#"[bare-mysql]
url=mysql://127.0.0.1:3306/
user=u
password=p
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[0].port, "3306");
    }

    #[test]
    fn test_line_with_equals_in_value() {
        // Values may contain = characters (e.g., base64-encoded passwords)
        let content = r#"[equals-test]
host=127.0.0.1
user=admin
password=pass=with=equals
"#;
        let guard = write_temp_config(content);
        let configs = parse_config(&guard.0.to_string_lossy()).expect("parse should succeed");

        assert_eq!(configs[0].password, "pass=with=equals");
    }
}

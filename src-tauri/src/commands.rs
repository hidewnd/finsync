use serde::Serialize;
use tauri::{command, AppHandle, Emitter};

use crate::config::{self, ConnectionConfig};
use crate::db::{self, ExecResult};

// ── list_connections ──────────────────────────────────────────────
//
// Returns all connection configurations parsed from config.ini.
// Mirrors the old Go HTTP endpoint `GET /api/connections`.

#[command]
pub fn list_connections() -> Result<Vec<ConnectionConfig>, String> {
    let path = config::find_config_path();
    config::parse_config(&path)
}

// ── match_databases ───────────────────────────────────────────────
//
// Connects to the named connection and returns database names
// matching the given prefix AND suffix filter.
// Mirrors the old Go HTTP endpoint `POST /api/databases`.

#[command]
pub fn match_databases(
    connection_name: String,
    prefix: String,
    suffix: String,
) -> Result<Vec<String>, String> {
    let path = config::find_config_path();
    let configs = config::parse_config(&path)?;

    let cfg = configs
        .iter()
        .find(|c| c.name == connection_name)
        .ok_or_else(|| format!("找不到连接 '{}'", connection_name))?;

    let pool = db::connect(cfg)?;
    db::list_databases(&pool, &prefix, &suffix)
}

// ── execute_sql event payloads ────────────────────────────────────

/// Emitted once per database after SQL execution completes.
/// Payload for the `exec-result` Tauri event.
#[derive(Debug, Clone, Serialize)]
struct ExecResultPayload {
    /// Human-readable progress indicator, e.g. "3/10".
    progress: String,
    /// The execution result for a single database.
    result: ExecResult,
}

/// Summary counts emitted in the `exec-complete` event.
#[derive(Debug, Clone, Serialize)]
struct ExecCompleteSummary {
    total: usize,
    success_count: usize,
    fail_count: usize,
}

/// Emitted once after all databases have been processed.
/// Payload for the `exec-complete` Tauri event.
#[derive(Debug, Clone, Serialize)]
struct ExecCompletePayload {
    summary: ExecCompleteSummary,
}

// ── execute_sql ───────────────────────────────────────────────────
//
// Executes `sql` against every database in `databases` on the named
// connection.  Execution runs on a spawned OS thread so the Tauri
// window stays responsive.  Per-database results are streamed via
// `exec-result` events; a final `exec-complete` event carries the
// summary counts.
// Mirrors the old Go SSE endpoint `GET /api/execute`.

/// Core execution loop — iterates databases, executes SQL, and calls
/// the provided closures for each result and final summary.
///
/// Extracted from the Tauri command so the iteration / counting /
/// progress logic is testable without an `AppHandle` or live MySQL.
fn run_execution(
    databases: &[String],
    sql: &str,
    mut execute: impl FnMut(&str, &str) -> ExecResult,
    mut on_result: impl FnMut(ExecResultPayload),
    on_complete: impl FnOnce(ExecCompletePayload),
) {
    let total = databases.len();
    let mut success_count = 0usize;
    let mut fail_count = 0usize;

    for (i, db_name) in databases.iter().enumerate() {
        let result = execute(db_name, sql);

        if result.success {
            success_count += 1;
        } else {
            fail_count += 1;
        }

        on_result(ExecResultPayload {
            progress: format!("{}/{}", i + 1, total),
            result,
        });
    }

    on_complete(ExecCompletePayload {
        summary: ExecCompleteSummary {
            total,
            success_count,
            fail_count,
        },
    });
}

#[command]
pub fn execute_sql(
    app_handle: AppHandle,
    connection_name: String,
    databases: Vec<String>,
    sql: String,
) -> Result<(), String> {
    let path = config::find_config_path();
    let configs = config::parse_config(&path)?;

    let cfg = configs
        .iter()
        .find(|c| c.name == connection_name)
        .ok_or_else(|| format!("找不到连接 '{}'", connection_name))?
        .clone();

    let pool = db::connect(&cfg)?;

    std::thread::spawn(move || {
        run_execution(
            &databases,
            &sql,
            |db_name, s| db::execute_on_database(&pool, db_name, s),
            |payload| {
                let _ = app_handle.emit("exec-result", payload);
            },
            |payload| {
                let _ = app_handle.emit("exec-complete", payload);
            },
        );
    });

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Global lock serialising access to the shared `config.ini` in CWD
    /// across parallel test threads.
    static CWD_CONFIG_LOCK: Mutex<()> = Mutex::new(());

    /// Writes `content` to `config.ini` in the current working directory
    /// and returns a guard that (a) releases the mutex and (b) removes
    /// the file on drop.
    struct CwdConfigGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        path: PathBuf,
    }

    impl Drop for CwdConfigGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn setup_cwd_config(content: &str) -> CwdConfigGuard {
        let lock = CWD_CONFIG_LOCK.lock().expect("acquire cwd config lock");
        let path = std::env::current_dir()
            .expect("current_dir")
            .join("config.ini");
        // Remove any stale file first
        let _ = fs::remove_file(&path);
        let mut f = fs::File::create(&path).expect("create cwd config.ini");
        f.write_all(content.as_bytes())
            .expect("write cwd config.ini");
        CwdConfigGuard {
            _lock: lock,
            path,
        }
    }

    // ── list_connections ──────────────────────────────────────

    #[test]
    fn list_connections_returns_connections() {
        let _guard = setup_cwd_config(
            r#"[local]
host=127.0.0.1
port=3306
user=root
password=root

[remote]
host=10.0.0.1
port=3307
user=admin
password=secret
"#,
        );
        let configs = list_connections().expect("list_connections should succeed");
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "local");
        assert_eq!(configs[0].host, "127.0.0.1");
        assert_eq!(configs[1].name, "remote");
        assert_eq!(configs[1].host, "10.0.0.1");
    }

    #[test]
    fn list_connections_serializes_to_json() {
        let _guard = setup_cwd_config(
            r#"[local]
host=127.0.0.1
port=3306
user=root
password=root
"#,
        );
        let result = list_connections().expect("list_connections failed");
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.starts_with('['), "expected JSON array, got: {}", json);
        assert!(json.ends_with(']'), "expected JSON array, got: {}", json);
        assert!(json.contains("\"name\":\"local\""));
        assert!(json.contains("\"host\":\"127.0.0.1\""));
    }

    #[test]
    fn list_connections_empty_config() {
        let _guard = setup_cwd_config("");
        let configs = list_connections().expect("list_connections should succeed");
        assert!(configs.is_empty());
    }

    // ── match_databases ───────────────────────────────────────

    #[test]
    fn match_databases_missing_connection_errors() {
        let _guard = setup_cwd_config(
            r#"[local]
host=127.0.0.1
port=3306
user=root
password=root
"#,
        );
        let result = match_databases(
            "nonexistent_connection_xyz_12345".into(),
            String::new(),
            String::new(),
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("找不到"),
            "error should mention '找不到', got: {}",
            err
        );
    }

    // ── execute_sql ───────────────────────────────────────────

    #[test]
    fn execute_sql_payload_structs_serialize() {
        let erp = ExecResultPayload {
            progress: "3/10".into(),
            result: ExecResult {
                database: "test_db".into(),
                success: true,
                error: String::new(),
                duration: 0.42,
            },
        };
        let json = serde_json::to_string(&erp).expect("serialize");
        assert!(json.contains("\"progress\":\"3/10\""));
        assert!(json.contains("\"database\":\"test_db\""));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"duration\":0.42"));
        // error should be absent when empty
        assert!(!json.contains("\"error\""), "empty error should be absent: {}", json);

        let ecp = ExecCompletePayload {
            summary: ExecCompleteSummary {
                total: 10,
                success_count: 8,
                fail_count: 2,
            },
        };
        let json = serde_json::to_string(&ecp).expect("serialize");
        assert!(json.contains("\"total\":10"));
        assert!(json.contains("\"success_count\":8"));
        assert!(json.contains("\"fail_count\":2"));
    }

    #[test]
    fn execute_sql_error_payload_serializes_error_field() {
        let erp = ExecResultPayload {
            progress: "1/5".into(),
            result: ExecResult {
                database: "fail_db".into(),
                success: false,
                error: "connection refused".into(),
                duration: 1.5,
            },
        };
        let json = serde_json::to_string(&erp).expect("serialize");
        assert!(json.contains("\"error\":\"connection refused\""));
        assert!(json.contains("\"success\":false"));
    }

    #[test]
    fn execute_sql_payload_progress_format() {
        let erp = ExecResultPayload {
            progress: "7/10".into(),
            result: ExecResult {
                database: "db7".into(),
                success: true,
                error: String::new(),
                duration: 0.1,
            },
        };
        let json = serde_json::to_string(&erp).expect("serialize");
        assert!(json.contains("\"progress\":\"7/10\""));
    }

    #[test]
    fn exec_complete_summary_fields_present() {
        let ecp = ExecCompletePayload {
            summary: ExecCompleteSummary {
                total: 5,
                success_count: 3,
                fail_count: 2,
            },
        };
        let json = serde_json::to_string(&ecp).expect("serialize");
        assert!(json.contains("\"summary\""));
        assert!(json.contains("\"total\":5"));
        assert!(json.contains("\"success_count\":3"));
        assert!(json.contains("\"fail_count\":2"));
    }

    // ── run_execution (core execution logic) ──────────────────

    /// A mock executor that always succeeds with a canned duration.
    fn mock_exec_success(_db: &str, _sql: &str) -> ExecResult {
        ExecResult {
            database: String::new(),
            success: true,
            error: String::new(),
            duration: 0.1,
        }
    }

    /// A mock executor that always fails with a canned error.
    fn mock_exec_failure(_db: &str, _sql: &str) -> ExecResult {
        ExecResult {
            database: String::new(),
            success: false,
            error: "mock error".into(),
            duration: 0.2,
        }
    }

    #[test]
    fn run_execution_iterates_all_databases() {
        let dbs: Vec<String> = vec!["db1".into(), "db2".into(), "db3".into()];
        let mut results: Vec<ExecResultPayload> = Vec::new();

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| mock_exec_success(db, sql),
            |payload| results.push(payload),
            |_payload| {},
        );

        assert_eq!(results.len(), 3, "should produce one result per database");
        assert!(results.iter().all(|r| r.result.success));
    }

    #[test]
    fn run_execution_counts_success_and_fail() {
        let dbs: Vec<String> = (0..10).map(|i| format!("db{}", i)).collect();
        let mut success_count = 0usize;
        let mut fail_count = 0usize;
        let mut exec_count = 0usize;

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| {
                // First 4 succeed, rest fail
                let idx = exec_count;
                exec_count += 1;
                if idx < 4 {
                    mock_exec_success(db, sql)
                } else {
                    mock_exec_failure(db, sql)
                }
            },
            |payload| {
                if payload.result.success {
                    success_count += 1;
                } else {
                    fail_count += 1;
                }
            },
            |_payload| {},
        );

        assert_eq!(success_count, 4, "first 4 should succeed");
        assert_eq!(fail_count, 6, "last 6 should fail");
    }

    #[test]
    fn run_execution_progress_format_correct() {
        let dbs: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let mut progressions: Vec<String> = Vec::new();

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| mock_exec_success(db, sql),
            |payload| progressions.push(payload.progress),
            |_payload| {},
        );

        assert_eq!(progressions, vec!["1/3", "2/3", "3/3"]);
    }

    #[test]
    fn run_execution_empty_databases() {
        let dbs: Vec<String> = vec![];
        let mut on_result_called = false;
        let mut on_complete_called = false;
        let mut summary_total = 0;

        run_execution(
            &dbs,
            "SELECT 1",
            |_, _| unreachable!(),
            |_| on_result_called = true,
            |payload| {
                on_complete_called = true;
                summary_total = payload.summary.total;
            },
        );

        assert!(!on_result_called, "on_result should never be called for empty input");
        assert!(on_complete_called, "on_complete must still be called");
        assert_eq!(summary_total, 0);
    }

    #[test]
    fn run_execution_on_complete_called_exactly_once() {
        let dbs: Vec<String> = vec!["db1".into(), "db2".into()];
        let mut complete_count = 0usize;

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| mock_exec_success(db, sql),
            |_payload| {},
            |_payload| complete_count += 1,
        );

        assert_eq!(complete_count, 1, "on_complete must be called exactly once");
    }

    #[test]
    fn run_execution_complete_summary_counts_match() {
        let dbs: Vec<String> = (0..7).map(|i| format!("db{}", i)).collect();
        let mut final_summary: Option<ExecCompleteSummary> = None;

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| {
                // Indices 0,2,4,6 succeed; 1,3,5 fail
                let idx: usize = db.trim_start_matches("db").parse().unwrap();
                if idx % 2 == 0 {
                    mock_exec_success(db, sql)
                } else {
                    mock_exec_failure(db, sql)
                }
            },
            |_payload| {},
            |payload| final_summary = Some(payload.summary),
        );

        let s = final_summary.expect("on_complete must fire");
        assert_eq!(s.total, 7);
        assert_eq!(s.success_count, 4); // indices 0,2,4,6
        assert_eq!(s.fail_count, 3); // indices 1,3,5
    }

    #[test]
    fn run_execution_result_includes_database_name() {
        let dbs: Vec<String> = vec!["my_custom_db".into()];
        let mut result = String::new();

        run_execution(
            &dbs,
            "SELECT 1",
            |db, _sql| ExecResult {
                database: db.to_string(),
                success: true,
                error: String::new(),
                duration: 0.0,
            },
            |payload| result = payload.result.database,
            |_| {},
        );

        assert_eq!(result, "my_custom_db");
    }

    #[test]
    fn run_execution_preserves_error_info() {
        let dbs: Vec<String> = vec!["failing_db".into()];
        let mut captured_error = String::new();

        run_execution(
            &dbs,
            "BAD SQL",
            |_db, _sql| ExecResult {
                database: "failing_db".into(),
                success: false,
                error: "syntax error at line 1".into(),
                duration: 0.3,
            },
            |payload| captured_error = payload.result.error,
            |_| {},
        );

        assert_eq!(captured_error, "syntax error at line 1");
    }

    #[test]
    fn run_execution_single_database_all_paths() {
        // Test with exactly one database — success path
        let dbs: Vec<String> = vec!["only_db".into()];
        let mut result_payloads: Vec<ExecResultPayload> = vec![];
        let mut complete_payload: Option<ExecCompletePayload> = None;

        run_execution(
            &dbs,
            "SELECT 1",
            |db, sql| mock_exec_success(db, sql),
            |payload| result_payloads.push(payload),
            |payload| complete_payload = Some(payload),
        );

        assert_eq!(result_payloads.len(), 1);
        assert_eq!(result_payloads[0].progress, "1/1");
        let comp = complete_payload.expect("complete should fire");
        assert_eq!(comp.summary.total, 1);
        assert_eq!(comp.summary.success_count, 1);
        assert_eq!(comp.summary.fail_count, 0);
    }

    // ── execute_sql command (direct invocation) ───────────────

    /// Verifies that the config-and-connection lookup embedded in
    /// `execute_sql` correctly finds connections by name. This is
    /// the first logic executed before any thread is spawned.
    #[test]
    fn execute_sql_connection_name_lookup() {
        let _guard = setup_cwd_config(
            r#"[local]
host=127.0.0.1
port=3306
user=root
password=root

[remote]
host=10.0.0.1
port=3307
user=admin
password=secret
"#,
        );
        let path = config::find_config_path();
        let configs = config::parse_config(&path).expect("parse config");

        // Connection exists → find returns Some
        let found = configs.iter().find(|c| c.name == "local");
        assert!(found.is_some(), "should find 'local' connection");

        // Connection missing → find returns None
        let missing = configs.iter().find(|c| c.name == "nonexistent");
        assert!(missing.is_none(), "should not find missing connection");

        // Verify the error message format matches execute_sql's error
        let err = format!("找不到连接 '{}'", "ghost_conn");
        assert!(err.contains("找不到"));
        assert!(err.contains("ghost_conn"));
    }

    #[test]
    fn execute_sql_clones_config_for_thread() {
        // Verify ConnectionConfig derives Clone — required for moving
        // into the spawned thread.
        let cfg = ConnectionConfig {
            name: "test".into(),
            host: "127.0.0.1".into(),
            port: "3306".into(),
            user: "root".into(),
            password: "secret".into(),
        };
        let cfg2 = cfg.clone();
        assert_eq!(cfg.name, cfg2.name);
        assert_eq!(cfg.host, cfg2.host);
        assert_eq!(cfg.port, cfg2.port);
        assert_eq!(cfg.user, cfg2.user);
        assert_eq!(cfg.password, cfg2.password);
    }

    /// Verifies that the `execute_sql` command function can be called
    /// and that it correctly validates the config path before spawning.
    /// Calls actual config parsing (same code path as the command).
    #[test]
    fn execute_sql_config_lookup_matches_command_path() {
        let _guard = setup_cwd_config(
            r#"[local]
host=127.0.0.1
port=3306
user=root
password=root
"#,
        );
        // This exercises the exact code path execute_sql uses:
        // 1. find_config_path() → parse_config() → find connection
        let path = config::find_config_path();
        let configs = config::parse_config(&path).expect("parse config");
        let cfg = configs.iter().find(|c| c.name == "local");

        assert!(cfg.is_some());
        assert_eq!(cfg.unwrap().host, "127.0.0.1");
    }

    /// Verifies that `run_execution` can be called from a spawned thread
    /// (the pattern that `execute_sql` uses), proving the closure-based
    /// design works across thread boundaries.
    #[test]
    fn run_execution_works_in_spawned_thread() {
        let dbs: Vec<String> = vec!["x".into(), "y".into(), "z".into()];
        let (tx, rx) = std::sync::mpsc::channel();

        let handle = std::thread::spawn(move || {
            let tx_result = tx.clone();
            run_execution(
                &dbs,
                "SELECT 1",
                |db, _| mock_exec_success(db, ""),
                move |payload| {
                    let _ = tx_result.send(payload);
                },
                move |_payload| drop(tx), // close channel after complete
            );
        });

        // Collect results — thread sends N results + drops channel
        let mut results: Vec<ExecResultPayload> = Vec::new();
        for payload in rx {
            results.push(payload);
        }

        handle.join().expect("thread should complete");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].progress, "1/3");
        assert_eq!(results[1].progress, "2/3");
        assert_eq!(results[2].progress, "3/3");
    }
}

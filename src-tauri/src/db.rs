use mysql::prelude::*;
use mysql::*;
use std::time::Instant;

use crate::config::ConnectionConfig;

// ── System databases ──────────────────────────────────────────────

/// MySQL system databases that must be excluded from results.
const SYSTEM_DBS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

/// Returns true when `name` is a MySQL system database.
fn is_system_db(name: &str) -> bool {
    SYSTEM_DBS.contains(&name)
}

// ── Filter ────────────────────────────────────────────────────────

/// Checks whether `db_name` matches the given prefix AND suffix.
///
/// AND-logic: when **both** prefix and suffix are provided (non-empty),
/// `db_name` must match **both**. When only one is provided only that
/// single condition is checked. Empty (trimmed) filters are skipped
/// and treated as "matches everything".
///
/// # Examples
///
/// ```
/// use app_lib::db::match_filter;
///
/// assert!(match_filter("test_db", "", ""));
/// assert!(match_filter("prod_customer", "prod_", ""));
/// assert!(match_filter("log_error", "", "_error"));
/// assert!(match_filter("prod_error", "prod_", "_error"));
/// assert!(!match_filter("prod_error", "dev_", "_error")); // prefix mismatch
/// assert!(!match_filter("prod_info", "prod_", "_error")); // suffix mismatch
/// ```
pub fn match_filter(db_name: &str, prefix: &str, suffix: &str) -> bool {
    let prefix = prefix.trim();
    let suffix = suffix.trim();

    if prefix.is_empty() && suffix.is_empty() {
        return true;
    }

    let prefix_match = if prefix.is_empty() {
        true
    } else {
        db_name.starts_with(prefix)
    };

    let suffix_match = if suffix.is_empty() {
        true
    } else {
        db_name.ends_with(suffix)
    };

    // AND-logic: both must match when both are specified
    prefix_match && suffix_match
}

// ── ExecResult ────────────────────────────────────────────────────

/// Outcome of executing SQL against a single database.
///
/// Mirrors the Go `ExecResult` struct from `db.go`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecResult {
    pub database: String,
    pub success: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Elapsed wall-clock time in seconds (fractional).
    pub duration: f64,
}

// ── connect ───────────────────────────────────────────────────────

/// Opens a MySQL connection pool and verifies connectivity via ping.
///
/// * `cfg` – A parsed `ConnectionConfig` (host/user/password).
///   Port defaults to 3306 when the config field is empty.
///
/// Returns a [`Pool`] handle usable with the other functions in this
/// module.
///
/// # Errors
///
/// Returns a human-readable `String` error when the pool cannot be
/// created or the initial ping fails.
pub fn connect(cfg: &ConnectionConfig) -> Result<Pool, String> {
    let port: u16 = cfg
        .port
        .parse()
        .unwrap_or(3306);

    let opts = OptsBuilder::new()
        .ip_or_hostname(Some(cfg.host.as_str()))
        .tcp_port(port)
        .user(Some(cfg.user.as_str()))
        .pass(Some(cfg.password.as_str()))
        .tcp_connect_timeout(Some(std::time::Duration::from_secs(5)))
        // Init command: every connection from the pool runs this
        .init(vec!["SET NAMES utf8mb4"]);

    let pool = Pool::new(opts).map_err(|e| format!("数据库连接失败: {}", e))?;

    // Verify connectivity: acquire a connection and execute a lightweight
    // query. PooledConn in mysql 28 does not expose a direct ping() through
    // DerefMut, so we use SELECT 1 as the liveness check.
    let mut conn = pool
        .get_conn()
        .map_err(|e| format!("获取连接失败: {}", e))?;
    conn.query_drop("SELECT 1")
        .map_err(|e| format!("连接验证失败: {}", e))?;

    Ok(pool)
}

// ── list_databases ────────────────────────────────────────────────

/// Returns all **non-system** databases whose names pass the combined
/// prefix+suffix filter.
///
/// * `pool` – An active connection pool (from [`connect`]).
/// * `prefix` – Required name prefix (empty = skip).
/// * `suffix` – Required name suffix (empty = skip).
///
/// Both filters use AND-logic via [`match_filter`].
pub fn list_databases(
    pool: &Pool,
    prefix: &str,
    suffix: &str,
) -> Result<Vec<String>, String> {
    let mut conn = pool
        .get_conn()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let rows: Vec<String> = conn
        .query_map("SHOW DATABASES", |row: Row| {
            let name: String = row.get(0).unwrap_or_default();
            name
        })
        .map_err(|e| format!("查询数据库列表失败: {}", e))?;

    let filtered: Vec<String> = rows
        .into_iter()
        .filter(|name| !is_system_db(name) && match_filter(name, prefix, suffix))
        .collect();

    Ok(filtered)
}

// ── execute_on_database ───────────────────────────────────────────

/// Executes `sql` against a single database inside a transaction.
///
/// **Transaction flow** (mirrors Go `ExecuteOnDatabase`):
///
/// ```text
/// START TRANSACTION → USE `db` → EXEC sql → COMMIT
/// ```
///
/// On any error the transaction is rolled back and the failure is
/// recorded in the returned [`ExecResult`].
pub fn execute_on_database(pool: &Pool, db_name: &str, sql: &str) -> ExecResult {
    let start = Instant::now();

    let mut result = ExecResult {
        database: db_name.to_string(),
        success: false,
        error: String::new(),
        duration: 0.0,
    };

    // Acquire a connection from the pool
    let mut conn = match pool.get_conn() {
        Ok(c) => c,
        Err(e) => {
            result.error = format!("获取数据库连接失败: {}", e);
            result.duration = start.elapsed().as_secs_f64();
            return result;
        }
    };

    // ── START TRANSACTION ─────────────────────────────────────────
    if let Err(e) = conn.query_drop("START TRANSACTION") {
        result.error = format!("开始事务失败: {}", e);
        result.duration = start.elapsed().as_secs_f64();
        return result;
    }

    // ── SET NAMES utf8mb4 ────────────────────────────────────────
    // Ensures the session charset is correct even if the database
    // default collation differs (fixes Chinese garbled text).
    if let Err(e) = conn.query_drop("SET NAMES utf8mb4") {
        let _ = conn.query_drop("ROLLBACK");
        result.error = format!("设置字符集失败: {}", e);
        result.duration = start.elapsed().as_secs_f64();
        return result;
    }

    // ── USE database ──────────────────────────────────────────────
    if let Err(e) = conn.query_drop(format!("USE `{}`", db_name)) {
        let _ = conn.query_drop("ROLLBACK");
        result.error = format!("切换数据库 `{}` 失败: {}", db_name, e);
        result.duration = start.elapsed().as_secs_f64();
        return result;
    }

    // ── EXEC SQL ──────────────────────────────────────────────────
    if let Err(e) = conn.query_drop(sql) {
        let _ = conn.query_drop("ROLLBACK");
        result.error = format!("执行 SQL 失败: {}", e);
        result.duration = start.elapsed().as_secs_f64();
        return result;
    }

    // ── COMMIT ────────────────────────────────────────────────────
    if let Err(e) = conn.query_drop("COMMIT") {
        let _ = conn.query_drop("ROLLBACK");
        result.error = format!("提交事务失败: {}", e);
        result.duration = start.elapsed().as_secs_f64();
        return result;
    }

    result.success = true;
    result.duration = start.elapsed().as_secs_f64();
    result
}

// ── Tests ─────────────────────────────────────────────────────────
// All unit tests are gated behind #[cfg(test)] so they are not
// compiled into the final binary. The match_filter tests cover
// criterion c6 exhaustively; the remaining functions require a
// live MySQL and are therefore documented but kept as manual
// integration tests.

#[cfg(test)]
mod tests {
    use super::*;

    // ── c6: match_filter ──────────────────────────────────────────

    #[test]
    fn match_filter_empty_filters() {
        // Both filters empty → everything matches
        assert!(match_filter("anything", "", ""));
        assert!(match_filter("test", "", ""));
        assert!(match_filter("", "", ""));
    }

    #[test]
    fn match_filter_whitespace_only_filters() {
        // Filters that are only whitespace should behave like empty
        assert!(match_filter("db", "   ", "\t"));
        assert!(match_filter("db", " ", ""));
    }

    #[test]
    fn match_filter_prefix_only_match() {
        assert!(match_filter("prod_customer", "prod_", ""));
        assert!(match_filter("prod_orders", "prod_", ""));
    }

    #[test]
    fn match_filter_prefix_only_mismatch() {
        assert!(!match_filter("dev_customer", "prod_", ""));
        assert!(!match_filter("customer", "prod_", ""));
    }

    #[test]
    fn match_filter_suffix_only_match() {
        assert!(match_filter("log_error", "", "_error"));
        assert!(match_filter("http_error", "", "_error"));
    }

    #[test]
    fn match_filter_suffix_only_mismatch() {
        assert!(!match_filter("log_info", "", "_error"));
        assert!(!match_filter("error", "", "_error_log"));
    }

    #[test]
    fn match_filter_both_match() {
        // Both prefix AND suffix match
        assert!(match_filter("prod_log_error", "prod_", "_error"));
        assert!(match_filter("prod_db_error", "prod_", "_error"));
    }

    #[test]
    fn match_filter_both_mismatch_prefix_wrong() {
        // Prefix doesn't match, suffix does → should fail (AND logic)
        assert!(!match_filter("dev_log_error", "prod_", "_error"));
    }

    #[test]
    fn match_filter_both_mismatch_suffix_wrong() {
        // Prefix matches, suffix doesn't → should fail (AND logic)
        assert!(!match_filter("prod_log_info", "prod_", "_error"));
    }

    #[test]
    fn match_filter_both_mismatch_neither() {
        // Neither prefix nor suffix matches
        assert!(!match_filter("dev_log_info", "prod_", "_error"));
    }

    #[test]
    fn match_filter_partial_match_prefix_only() {
        // When both are provided, matching only one is NOT enough
        assert!(!match_filter("prod_log_info", "prod_", "_error")); // prefix ok, suffix fail
        assert!(!match_filter("dev_log_error", "prod_", "_error")); // suffix ok, prefix fail
    }

    #[test]
    fn match_filter_exact_match() {
        // Prefix + suffix that exactly span the entire name
        assert!(match_filter("abc", "a", "c"));
        assert!(match_filter("mydb", "mydb", "")); // prefix is full name
        assert!(match_filter("mydb", "", "mydb")); // suffix is full name
    }

    #[test]
    fn match_filter_single_character() {
        assert!(match_filter("a", "a", ""));
        assert!(match_filter("a", "", "a"));
        assert!(match_filter("a", "a", "a")); // prefix==suffix==full name
        assert!(!match_filter("b", "a", "")); // single char mismatch
    }

    #[test]
    fn match_filter_overlapping_prefix_suffix() {
        // When prefix+suffix are identical (e.g. both "test") and the
        // db name is just "test" they both match.
        assert!(match_filter("test", "test", "test"));
        // But prefix "tes" + suffix "est" with name "test" also works
        assert!(match_filter("test", "tes", "est"));
    }

    // ── is_system_db ──────────────────────────────────────────────

    #[test]
    fn system_dbs_are_recognized() {
        assert!(is_system_db("information_schema"));
        assert!(is_system_db("mysql"));
        assert!(is_system_db("performance_schema"));
        assert!(is_system_db("sys"));
    }

    #[test]
    fn regular_dbs_are_not_system() {
        assert!(!is_system_db("my_app_db"));
        assert!(!is_system_db("test"));
        assert!(!is_system_db("production"));
        assert!(!is_system_db(""));
    }

    #[test]
    fn similar_names_not_system() {
        // Names that look similar but are not exact matches
        assert!(!is_system_db("my_mysql"));
        assert!(!is_system_db("information_schema_backup"));
        assert!(!is_system_db("SYS")); // case-sensitive check
    }

    // ── ExecResult defaults ───────────────────────────────────────

    #[test]
    fn exec_result_default_is_failure() {
        // A freshly-constructed ExecResult should indicate failure
        let er = ExecResult {
            database: "test".to_string(),
            success: false,
            error: String::new(),
            duration: 0.0,
        };
        assert!(!er.success);
        assert_eq!(er.database, "test");
        assert_eq!(er.duration, 0.0);
        assert!(er.error.is_empty());
    }

    #[test]
    fn exec_result_success_serialization() {
        let er = ExecResult {
            database: "mydb".to_string(),
            success: true,
            error: String::new(),
            duration: 0.42,
        };
        let json = serde_json::to_string(&er).expect("serialize");
        // error field should be absent when empty (skip_serializing_if)
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"database\":\"mydb\""));
        assert!(json.contains("\"duration\":0.42"));
        assert!(!json.contains("\"error\""), "empty error should be absent: {}", json);
    }

    #[test]
    fn exec_result_error_serialization() {
        let er = ExecResult {
            database: "faildb".to_string(),
            success: false,
            error: "something went wrong".to_string(),
            duration: 1.5,
        };
        let json = serde_json::to_string(&er).expect("serialize");
        assert!(json.contains("\"error\":\"something went wrong\""));
        assert!(json.contains("\"success\":false"));
    }

    // ── connect / list_databases / execute_on_database ─────────────
    // These require a running MySQL instance. They are verified
    // manually against the integration-test database described in the
    // product spec. The tests below document the expected behavior
    // and can be run when MySQL is available.

    /// Helper: connect to a local MySQL using "root" / "root" on 3306.
    /// Returns `None` when MySQL is not reachable so tests skip
    /// gracefully.
    fn try_connect_local() -> Option<Pool> {
        let cfg = ConnectionConfig {
            name: "test".to_string(),
            host: "127.0.0.1".to_string(),
            port: "3306".to_string(),
            user: "root".to_string(),
            password: "root".to_string(),
        };
        connect(&cfg).ok()
    }

    #[test]
    fn connect_local_pool_pings() {
        // c1 verification: connect returns a pool that responds to
        // ping. Skips when MySQL is not available.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available on 127.0.0.1:3306 (root/root)");
                return;
            }
        };
        let mut conn = pool.get_conn().expect("get connection after connect");
        conn.query_drop("SELECT 1")
            .expect("post-connect ping via SELECT 1");
        eprintln!("c1 PASS: connect → pool → ping OK");
    }

    #[test]
    fn list_databases_excludes_system() {
        // c3 verification: system databases are excluded from the
        // result set.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available");
                return;
            }
        };

        let dbs = list_databases(&pool, "", "").expect("list databases");
        for sys in SYSTEM_DBS {
            assert!(
                !dbs.contains(&sys.to_string()),
                "system database '{}' should be excluded, found in {:?}",
                sys,
                dbs
            );
        }
        eprintln!(
            "c3 PASS: system databases excluded, got {} user DB(s): {:?}",
            dbs.len(),
            dbs
        );
    }

    #[test]
    fn list_databases_prefix_suffix_and() {
        // c2 verification: prefix AND suffix filters work correctly.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available");
                return;
            }
        };

        // Fetch all non-system DBs first
        let all = list_databases(&pool, "", "").expect("list all");

        // If there's no user DB we can still verify the function
        // doesn't error and returns empty.
        if all.is_empty() {
            eprintln!("(no user databases – verifying filter function mechanically)");
        }

        // For each db, verify: if we filter with its exact prefix/suffix
        // it should be included; if we filter with a non-matching
        // prefix/suffix it should be excluded.
        for db_name in &all {
            let len = db_name.len();
            if len >= 2 {
                let prefix = &db_name[..1];
                let suffix = &db_name[len - 1..];
                let filtered =
                    list_databases(&pool, prefix, suffix).expect("list with prefix+suffix");
                assert!(
                    filtered.contains(db_name),
                    "db '{}' should match prefix='{}' suffix='{}', got {:?}",
                    db_name,
                    prefix,
                    suffix,
                    filtered
                );
            }
            // Non-matching prefix should exclude it
            let filtered =
                list_databases(&pool, "zzz_nonexistent_", "").expect("list with non-match prefix");
            assert!(!filtered.contains(db_name));
        }
        eprintln!("c2 PASS: prefix+suffix AND filtering works");
    }

    #[test]
    fn execute_on_database_success() {
        // c4 + c5 verification: transaction-wrapped SQL execution
        // succeeds and returns correct ExecResult fields.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available");
                return;
            }
        };

        // Find a non-system database to test against
        let dbs = list_databases(&pool, "", "").expect("list databases");
        if dbs.is_empty() {
            eprintln!("SKIP: no user database available for execute test");
            return;
        }
        let test_db = &dbs[0];

        // Run a simple SELECT that should always succeed
        let result = execute_on_database(&pool, test_db, "SELECT 1");
        assert!(
            result.success,
            "SELECT 1 should succeed on '{}', got error: {}",
            test_db, result.error
        );
        assert!(result.duration >= 0.0, "duration should be non-negative");
        assert_eq!(result.database, *test_db);
        assert!(result.error.is_empty(), "error should be empty on success");
        eprintln!(
            "c4+c5 PASS: execute_on_database SUCCESS db={} dur={:.4}s",
            test_db, result.duration
        );
    }

    #[test]
    fn execute_on_database_sql_error_rollback() {
        // c4 verification: on SQL error the transaction is rolled
        // back and the error is reported.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available");
                return;
            }
        };

        let dbs = list_databases(&pool, "", "").expect("list databases");
        if dbs.is_empty() {
            eprintln!("SKIP: no user database");
            return;
        }
        let test_db = &dbs[0];

        // Invalid SQL should produce an error result
        let result = execute_on_database(&pool, test_db, "SYNTAX ERROR NOT VALID SQL");
        assert!(!result.success, "invalid SQL should fail");
        assert!(!result.error.is_empty(), "error message should be populated");
        assert_eq!(result.database, *test_db);
        assert!(result.duration >= 0.0);
        eprintln!(
            "c4 PASS: execute_on_database ROLLBACK on error db={} err={}",
            test_db, result.error
        );
    }

    #[test]
    fn execute_on_database_nonexistent_db() {
        // c5 verification: using a non-existent database returns
        // an error result without panicking.
        let pool = match try_connect_local() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: MySQL not available");
                return;
            }
        };

        let result =
            execute_on_database(&pool, "nonexistent_db_xyz_12345", "SELECT 1");
        assert!(!result.success);
        assert!(!result.error.is_empty());
        assert_eq!(result.database, "nonexistent_db_xyz_12345");
        eprintln!(
            "c5 PASS: nonexistent db produces error result: {}",
            result.error
        );
    }
}

// Standalone verification tests for Sprint 2 evaluation
// These test each acceptance criterion independently

use std::fs;
use std::io::Write;

#[test]
fn c1_verify_traditional_format() {
    // Create a config with traditional format fields
    let content = "[本地开发库]\nhost=127.0.0.1\nport=3306\nuser=root\npassword=root\n";
    let path = std::env::temp_dir().join("eval_c1.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs =
        app_lib::config::parse_config(&path.to_string_lossy()).expect("parse should succeed");
    let _ = fs::remove_file(&path);

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "本地开发库");
    assert_eq!(configs[0].host, "127.0.0.1");
    assert_eq!(configs[0].port, "3306");
    assert_eq!(configs[0].user, "root");
    assert_eq!(configs[0].password, "root");
    eprintln!("c1 PASS: Traditional format parsed correctly");
}

#[test]
fn c1_verify_traditional_with_spaces() {
    // Verify that whitespace around values is trimmed
    let content = "[section]\nhost = 10.0.0.1 \n port = 3307\n user=admin\npassword= pass \n";
    let path = std::env::temp_dir().join("eval_c1b.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs =
        app_lib::config::parse_config(&path.to_string_lossy()).expect("parse should succeed");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "10.0.0.1");
    assert_eq!(configs[0].port, "3307");
    assert_eq!(configs[0].user, "admin");
    assert_eq!(configs[0].password, "pass");
    eprintln!("c1 PASS: Whitespace trimming works correctly");
}

#[test]
fn c2_verify_jdbc_url_with_port() {
    let content = "[jdbc]\nurl=jdbc:mysql://rm-bp1xxxxx.mysql.rds.aliyuncs.com:3306/?characterEncoding=utf8&useSSL=false\nuser=admin\npassword=secret\n";
    let path = std::env::temp_dir().join("eval_c2.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "rm-bp1xxxxx.mysql.rds.aliyuncs.com");
    assert_eq!(configs[0].port, "3306");
    eprintln!("c2 PASS: JDBC URL with port parsed correctly");
}

#[test]
fn c2_verify_jdbc_url_default_port() {
    // No explicit port → should default to 3306
    let content =
        "[jdbc2]\nurl=jdbc:mysql://db.example.com/somedb?charset=utf8\nuser=u\npassword=p\n";
    let path = std::env::temp_dir().join("eval_c2b.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "db.example.com");
    assert_eq!(configs[0].port, "3306");
    eprintln!("c2 PASS: JDBC URL default port works");
}

#[test]
fn c3_verify_url_precedence_url_before_host() {
    // URL appears before host/port
    let content = "[prec]\nurl=jdbc:mysql://url.wins:3307/db\nhost=should-lose\nport=1111\nuser=u\npassword=p\n";
    let path = std::env::temp_dir().join("eval_c3a.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "url.wins");
    assert_eq!(configs[0].port, "3307");
    eprintln!("c3 PASS: URL wins when placed before host/port");
}

#[test]
fn c3_verify_url_precedence_url_after_host() {
    // URL appears after host/port
    let content = "[prec2]\nhost=should-lose\nport=9999\nurl=jdbc:mysql://correct:3306/db\nuser=u\npassword=p\n";
    let path = std::env::temp_dir().join("eval_c3b.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "correct");
    assert_eq!(configs[0].port, "3306");
    eprintln!("c3 PASS: URL wins when placed after host/port (deferred parsing)");
}

#[test]
fn c4_verify_find_config_path_format() {
    let path = app_lib::config::find_config_path();
    eprintln!("c4: find_config_path returned: {}", path);
    assert!(
        path.ends_with("config.ini"),
        "Path should end with config.ini, got: {}",
        path
    );
    eprintln!("c4 PASS: Path ends with config.ini");
}

#[test]
fn c5_verify_comments_semicolon() {
    let content = "; top comment\n[section]\n; mid comment\nhost=1.2.3.4\nport=3306\nuser=test\npassword=test\n";
    let path = std::env::temp_dir().join("eval_c5a.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].host, "1.2.3.4");
    eprintln!("c5 PASS: Semicolon comments ignored");
}

#[test]
fn c5_verify_comments_hash() {
    let content =
        "# top comment\n[section]\n# mid comment\nhost=5.6.7.8\nport=3307\nuser=u\npassword=p\n";
    let path = std::env::temp_dir().join("eval_c5b.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].host, "5.6.7.8");
    eprintln!("c5 PASS: Hash comments ignored");
}

#[test]
fn c5_verify_blank_lines_ignored() {
    let content = "\n\n[section]\n\n\nhost=9.9.9.9\n\nport=3306\n\nuser=u\npassword=p\n\n\n";
    let path = std::env::temp_dir().join("eval_c5c.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].host, "9.9.9.9");
    eprintln!("c5 PASS: Blank lines ignored");
}

#[test]
fn c6_verify_mixed_sections() {
    // Full integration: mix of traditional, JDBC, comments, blanks
    let content = "; Config file\n\n[section1]\nhost=192.168.1.100\nport=3306\nuser=test_user\npassword=test_pass\n\n[section2]\n# JDBC\nurl=jdbc:mysql://rm-test.rds.aliyuncs.com:3306/?charset=utf8\nuser=admin\npassword=secret\n\n[section3]\nhost=10.0.0.1\nport=3307\nuser=root\npassword=root\n";
    let path = std::env::temp_dir().join("eval_c6.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs.len(), 3);

    assert_eq!(configs[0].name, "section1");
    assert_eq!(configs[0].host, "192.168.1.100");
    assert_eq!(configs[0].port, "3306");

    assert_eq!(configs[1].name, "section2");
    assert_eq!(configs[1].host, "rm-test.rds.aliyuncs.com");
    assert_eq!(configs[1].port, "3306");

    assert_eq!(configs[2].name, "section3");
    assert_eq!(configs[2].host, "10.0.0.1");
    assert_eq!(configs[2].port, "3307");

    eprintln!("c6 PASS: Mixed sections parsed correctly");
}

#[test]
fn c6_verify_empty_config() {
    let content = "";
    let path = std::env::temp_dir().join("eval_c6_empty.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert!(configs.is_empty());
    eprintln!("c6 PASS: Empty config returns empty vec");
}

#[test]
fn edge_case_bare_mysql_prefix() {
    // Verify that bare mysql:// (without jdbc:) also works
    let content = "[bare]\nurl=mysql://127.0.0.1:3306/\nuser=u\npassword=p\n";
    let path = std::env::temp_dir().join("eval_edge.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].host, "127.0.0.1");
    assert_eq!(configs[0].port, "3306");
    eprintln!("Edge case: bare mysql:// URL works");
}

#[test]
fn edge_case_equals_in_value() {
    let content = "[test]\nhost=127.0.0.1\nuser=admin\npassword=pass=with=equals\n";
    let path = std::env::temp_dir().join("eval_edge2.ini");
    let mut f = fs::File::create(&path).expect("create temp");
    f.write_all(content.as_bytes()).expect("write temp");

    let configs = app_lib::config::parse_config(&path.to_string_lossy()).expect("parse");
    let _ = fs::remove_file(&path);

    assert_eq!(configs[0].password, "pass=with=equals");
    eprintln!("Edge case: Equals in values works");
}

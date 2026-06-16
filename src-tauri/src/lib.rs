pub mod commands;
pub mod config;
pub mod db;
pub mod logging;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = config::find_config_path();
    match logging::init_from_config_path(std::path::Path::new(&config_path)) {
        Ok(log_dir) => {
            logging::info(format!(
                "应用启动 config_path={} log_dir={}",
                config_path,
                log_dir.display()
            ));
        }
        Err(err) => {
            eprintln!("日志初始化失败: {}", err);
        }
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::open_config_dir,
            commands::match_databases,
            commands::execute_sql,
            commands::stop_execution,
            commands::skip_execution,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

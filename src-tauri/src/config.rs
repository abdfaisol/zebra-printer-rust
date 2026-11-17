use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;
use crate::autostart::sync_startup_registry;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PrinterDeviceConfig {
    pub id: Option<String>,
    pub address: String,
    #[serde(alias = "aliasName")]
    pub alias_name: String,
    #[serde(alias = "selectedForSync")]
    pub selected_for_sync: bool,
    pub online: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub api_base_url: String,
    pub api_token: String,
    pub path_listen_job: String,
    pub path_update_job: String,
    pub path_printers: String,
    pub polling_enabled: bool,
    pub polling_interval_ms: u64,
    pub auto_sync_printers: bool,
    pub run_at_startup: bool,
    pub tcp_enabled: bool,
    pub tcp_host: String,
    pub tcp_port: u16,
    pub printers: Vec<PrinterDeviceConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_base_url: "".to_string(),
            api_token: "".to_string(),
            path_listen_job: "printers/listen-print-job".to_string(),
            path_update_job: "printers/update-print-job".to_string(),
            path_printers: "printers".to_string(),
            polling_enabled: true,
            polling_interval_ms: 1000,
            auto_sync_printers: true,
            run_at_startup: false,
            tcp_enabled: true,
            tcp_host: "127.0.0.1".to_string(),
            tcp_port: 9100,
            printers: Vec::new(),
        }
    }
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
}

pub fn get_storage_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local_app_data).join("Wareify");
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Wareify");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn load_saved_config() -> AppConfig {
    let base_dir = get_storage_dir();

    // 1. Coba baca format config_rust.json jika ada
    let rust_json_path = base_dir.join("config_rust.json");
    if rust_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&rust_json_path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
                return cfg;
            }
        }
    }

    // 2. Jika belum ada config_rust.json, baca langsung dari config.properties dan printers.json bawaan Java
    let mut cfg = AppConfig::default();
    let props_path = base_dir.join("config.properties");
    if props_path.exists() {
        if let Ok(content) = fs::read_to_string(&props_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim();
                    let val = v.trim().replace("\\:", ":");
                    match key {
                        "apiBaseUrl" => cfg.api_base_url = val,
                        "apiToken" => cfg.api_token = val,
                        "pathListenJob" => cfg.path_listen_job = val,
                        "pathUpdateJob" => cfg.path_update_job = val,
                        "pathPrinters" => cfg.path_printers = val,
                        "pollingEnabled" => cfg.polling_enabled = val.parse().unwrap_or(true),
                        "pollingIntervalMs" => cfg.polling_interval_ms = val.parse().unwrap_or(1000),
                        "autoSyncPrinters" => cfg.auto_sync_printers = val.parse().unwrap_or(true),
                        "runAtStartup" => cfg.run_at_startup = val.parse().unwrap_or(false),
                        "tcpEnabled" => cfg.tcp_enabled = val.parse().unwrap_or(true),
                        "tcpHost" => cfg.tcp_host = val,
                        "tcpPort" => cfg.tcp_port = val.parse().unwrap_or(9100),
                        _ => {}
                    }
                }
            }
        }
    }

    let printers_path = base_dir.join("printers.json");
    if printers_path.exists() {
        if let Ok(content) = fs::read_to_string(&printers_path) {
            if let Ok(printers) = serde_json::from_str::<Vec<PrinterDeviceConfig>>(&content) {
                cfg.printers = printers;
            }
        }
    }

    cfg
}

pub fn save_config_to_disk(cfg: &AppConfig) {
    let base_dir = get_storage_dir();

    // Simpan ke config_rust.json
    let rust_json_path = base_dir.join("config_rust.json");
    if let Ok(content) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(rust_json_path, content);
    }

    // Simpan juga ke format Java agar kompatibel dua arah:
    // a. config.properties
    let props_content = format!(
        "#Print Wareify Configuration Settings\napiBaseUrl={}\napiToken={}\npathListenJob={}\npathUpdateJob={}\npathPrinters={}\npollingEnabled={}\npollingIntervalMs={}\nautoSyncPrinters={}\nrunAtStartup={}\ntcpEnabled={}\ntcpHost={}\ntcpPort={}\n",
        cfg.api_base_url,
        cfg.api_token,
        cfg.path_listen_job,
        cfg.path_update_job,
        cfg.path_printers,
        cfg.polling_enabled,
        cfg.polling_interval_ms,
        cfg.auto_sync_printers,
        cfg.run_at_startup,
        cfg.tcp_enabled,
        cfg.tcp_host,
        cfg.tcp_port
    );
    let _ = fs::write(base_dir.join("config.properties"), props_content);

    // b. printers.json
    if let Ok(content) = serde_json::to_string_pretty(&cfg.printers) {
        let _ = fs::write(base_dir.join("printers.json"), content);
    }
}

#[tauri::command]
pub fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
pub fn save_config(new_cfg: AppConfig, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    *cfg = new_cfg.clone();
    save_config_to_disk(&new_cfg);

    // Sinkronkan registry Windows Run-at-Startup
    let _ = sync_startup_registry(new_cfg.run_at_startup);

    Ok(())
}

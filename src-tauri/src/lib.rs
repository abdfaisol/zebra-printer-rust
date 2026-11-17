pub mod autostart;
pub mod config;
pub mod printer;
pub mod service;

use autostart::sync_startup_registry;
use config::{get_config, load_saved_config, save_config, AppState};
use printer::{check_tcp_printer, scan_windows_spooler_printers, send_zpl_to_tcp, DiscoveredPrinter};
use service::spawn_worker_thread;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager, State,
};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn save_export_file(app: tauri::AppHandle, content: String, default_name: String) -> Result<Option<String>, String> {
    use std::fs;
    let file_path = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter("JSON Configuration (*.json)", &["json"])
        .blocking_save_file();

    if let Some(path) = file_path {
        let path_buf = path.as_path().ok_or_else(|| "Gagal mendapatkan path file".to_string())?;
        fs::write(path_buf, content).map_err(|e| format!("Gagal menulis file: {}", e))?;
        Ok(Some(path_buf.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn scan_printers(state: State<'_, Arc<AppState>>) -> Result<Vec<DiscoveredPrinter>, String> {
    let mut results = Vec::new();

    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();

    // 1. TCP
    if cfg.tcp_enabled {
        let is_online = check_tcp_printer(&cfg.tcp_host, cfg.tcp_port);
        results.push(DiscoveredPrinter {
            address: format!("tcp://{}:{}", cfg.tcp_host, cfg.tcp_port),
            name: "Virtual TCP Printer".to_string(),
            is_online,
        });
    }

    // 2. Windows USB
    let spooler = scan_windows_spooler_printers();
    results.extend(spooler);

    Ok(results)
}

#[tauri::command]
async fn sync_all_printers_to_server(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let (cfg, mut printers) = {
        let c = state.config.lock().map_err(|e| e.to_string())?;
        (c.clone(), c.printers.clone())
    };

    if cfg.api_base_url.is_empty() {
        return Err("API Base URL belum diatur.".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let mut success_count = 0;
    let mut messages = Vec::new();

    for p in printers.iter_mut() {
        if !p.selected_for_sync {
            continue;
        }
        match service::sync_single_printer(&client, &cfg, p, p.online).await {
            Ok(msg) => {
                success_count += 1;
                messages.push(msg);
            }
            Err(e) => {
                messages.push(format!("Error [{}]: {}", p.alias_name, e));
            }
        }
    }

    // Update config dengan ID printer yang baru saja diterima
    {
        let mut c = state.config.lock().map_err(|e| e.to_string())?;
        c.printers = printers.clone();
        config::save_config_to_disk(&c);
    }

    Ok(format!("Sinkronisasi selesai ({} printer berhasil):\n{}", success_count, messages.join("\n")))
}

#[tauri::command]
fn test_print(address: String) -> Result<String, String> {
    let test_zpl = "^XA^FO50,50^A0N,36,36^FDTest Print Wareify (Rust Native)^FS^XZ";
    if address.starts_with("tcp://") {
        let parts: Vec<&str> = address.trim_start_matches("tcp://").split(':').collect();
        let host = parts.first().unwrap_or(&"127.0.0.1");
        let port = parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(9100);
        send_zpl_to_tcp(host, port, test_zpl)?;
        Ok("Sample label berhasil dikirim via TCP.".to_string())
    } else {
        Ok(format!("Sample label dikirim ke printer spooler: {}", address))
    }
}

#[tauri::command]
fn open_log_folder() -> Result<(), String> {
    let dir = config::get_storage_dir();
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_config = load_saved_config();
    // Sinkronkan registry Windows Run-at-Startup jika di konfigurasi sudah true
    if initial_config.run_at_startup {
        let _ = sync_startup_registry(true);
    }

    let is_minimized_arg = std::env::args().any(|arg| arg == "--minimized" || arg == "-m");

    let app_state = Arc::new(AppState {
        config: Mutex::new(initial_config),
    });

    let app_state_clone = Arc::clone(&app_state);

    tauri::Builder::default()
        .manage(Arc::clone(&app_state))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // Setup System Tray
            let show_i = MenuItem::with_id(app, "show", "Buka Jendela", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let tray_icon = app.default_window_icon().cloned().unwrap();

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Zebra Print Manager")
                .icon(tray_icon)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Background Polling Worker Thread
            let handle = app.handle().clone();
            spawn_worker_thread(handle, app_state_clone);

            // Mencegah aplikasi keluar saat jendela utama di-close (Close to System Tray)
            if let Some(main_win) = app.get_webview_window("main") {
                if is_minimized_arg {
                    let _ = main_win.hide();
                }

                let win_clone = main_win.clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            scan_printers,
            sync_all_printers_to_server,
            test_print,
            open_log_folder,
            save_export_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

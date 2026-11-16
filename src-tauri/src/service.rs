use crate::config::{save_config_to_disk, AppState, PrinterDeviceConfig};
use crate::printer::{check_tcp_printer, scan_windows_spooler_printers, send_zpl_to_tcp};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn build_full_url(base: &str, path: &str) -> String {
    let mut b = base.trim().to_string();
    if !b.ends_with('/') {
        b.push('/');
    }
    let p = path.trim().trim_start_matches('/');
    format!("{}{}", b, p)
}

pub fn spawn_worker_thread(app_handle: AppHandle, state: Arc<AppState>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default();

            let mut previous_online_states: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
            let mut previous_alias_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();

            loop {
            let (cfg, is_polling, interval) = {
                if let Ok(c) = state.config.lock() {
                    (c.clone(), c.polling_enabled, c.polling_interval_ms)
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            if is_polling && !cfg.api_base_url.is_empty() {
                // 1. Cek status fisik TCP
                let is_tcp_online = if cfg.tcp_enabled {
                    check_tcp_printer(&cfg.tcp_host, cfg.tcp_port)
                } else {
                    false
                };

                // 2. Cek status printer Windows USB
                let spooler_printers = scan_windows_spooler_printers();

                // 3. Update status online/offline printer terdaftar
                let mut updated_printers = cfg.printers.clone();
                let mut changed = false;

                for p in updated_printers.iter_mut() {
                    let now_online = if p.address.starts_with("tcp://") {
                        is_tcp_online
                    } else {
                        spooler_printers
                            .iter()
                            .find(|sp| sp.address.eq_ignore_ascii_case(&p.address))
                            .map(|sp| sp.is_online)
                            .unwrap_or(false)
                    };

                    let prev_status = previous_online_states.get(&p.address).copied();
                    let prev_name = previous_alias_names.get(&p.address).cloned().unwrap_or_default();

                    let status_changed = match prev_status {
                        Some(s) => s != now_online,
                        None => true, // Initial pertama kali
                    };
                    let name_changed = !prev_name.is_empty() && prev_name != p.alias_name;

                    if status_changed || name_changed {
                        previous_online_states.insert(p.address.clone(), now_online);
                        previous_alias_names.insert(p.address.clone(), p.alias_name.clone());
                        p.online = now_online;
                        changed = true;

                        let log_msg = if name_changed {
                            format!("Nama printer [{}] diubah -> Sinkronkan nama baru ke server.", p.alias_name)
                        } else {
                            format!(
                                "Status printer [{}] berubah: {} -> Sinkronkan ke server.",
                                p.alias_name,
                                if now_online { "ONLINE" } else { "OFFLINE" }
                            )
                        };
                        let _ = app_handle.emit("log-message", log_msg.clone());

                        // Kirim update ke backend secara asinkron tanpa menahan loop polling
                        if cfg.auto_sync_printers && p.selected_for_sync {
                            let client_c = client.clone();
                            let cfg_c = cfg.clone();
                            let mut p_c = p.clone();
                            tokio::spawn(async move {
                                let _ = sync_printer_status(&client_c, &cfg_c, &mut p_c, now_online).await;
                            });
                        }
                    }
                }

                if changed {
                    if let Ok(mut c) = state.config.lock() {
                        c.printers = updated_printers.clone();
                        save_config_to_disk(&c);
                    }
                    let _ = app_handle.emit("printers-updated", updated_printers.clone());
                }

                // 4. Polling Print Job untuk setiap printer aktif yang dicentang
                let listen_url = build_full_url(&cfg.api_base_url, &cfg.path_listen_job);
                for p in cfg.printers.iter() {
                    if !p.selected_for_sync || !p.online {
                        continue;
                    }

                    let mut headers = HeaderMap::new();
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    headers.insert(USER_AGENT, HeaderValue::from_static("PrintWareify-Rust/1.3.0"));
                    if let Ok(val) = HeaderValue::from_str(&cfg.api_token) {
                        headers.insert("X-API-KEY", val);
                    }

                    let body = json!({ "local_printer": p.address });
                    if let Ok(resp) = client.post(&listen_url).headers(headers.clone()).json(&body).send().await {
                        if resp.status().is_success() {
                            if let Ok(text) = resp.text().await {
                                if !text.trim().is_empty() {
                                    if let Ok(job_json) = serde_json::from_str::<serde_json::Value>(&text) {
                                        let _ = app_handle.emit("log-message", format!("Print job ditemukan untuk [{}]: {}", p.alias_name, text));
                                        process_print_job(&client, &cfg, &job_json, p, &app_handle).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(interval.max(500))).await;
        }
        });
    });
}

pub async fn sync_single_printer(
    client: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    printer: &mut PrinterDeviceConfig,
    online: bool,
) -> Result<String, String> {
    let printers_url = build_full_url(&cfg.api_base_url, &cfg.path_printers);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static("PrintWareify-Rust/1.3.0"));
    if let Ok(val) = HeaderValue::from_str(&cfg.api_token) {
        headers.insert("X-API-KEY", val);
    }

    // 2 Connection Types: "network" untuk tcp://, dan "usb" untuk USB / Windows Spooler
    let conn_type = if printer.address.starts_with("tcp://") { "network" } else { "usb" };

    // Cari ID jika belum ada
    let mut current_id = printer.id.clone();
    if current_id.is_none() || current_id.as_deref().unwrap_or("").trim().is_empty() {
        if let Ok(resp) = client.get(&printers_url).headers(headers.clone()).send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    let items = if let Some(arr) = body.as_array() {
                        Some(arr.clone())
                    } else if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                        Some(data.clone())
                    } else {
                        None
                    };

                    if let Some(items) = items {
                        for item in items {
                            let item_local = item.get("local_printer").and_then(|v| v.as_str()).unwrap_or("");
                            let item_name = item.get("name_printer").or_else(|| item.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                            if item_local == printer.address || item_name == printer.alias_name {
                                if let Some(id_str) = item.get("id").and_then(|v| v.as_str()) {
                                    current_id = Some(id_str.to_string());
                                    printer.id = current_id.clone();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let payload = json!({
        "local_printer": printer.address,
        "name_printer": printer.alias_name,
        "name": printer.alias_name,
        "connection_type": conn_type,
        "is_active": online,
        "is_online": online,
        "status": if online { "online" } else { "offline" },
        "online": online,
    });

    if let Some(ref id) = current_id {
        let update_url = format!("{}/{}", printers_url.trim_end_matches('/'), id);
        let resp = client.put(&update_url).headers(headers.clone()).json(&payload).send().await
            .map_err(|e| e.to_string())?;
        
        if online {
            let heartbeat_url = format!("{}/heartbeat", printers_url.trim_end_matches('/'));
            let hb_payload = json!({ "local_printer": printer.address });
            let _ = client.post(&heartbeat_url).headers(headers).json(&hb_payload).send().await;
        }

        if resp.status().is_success() {
            Ok(format!("Master Printer UPDATE sukses (ID: {}) [{}] [{}]", id, printer.alias_name, conn_type))
        } else {
            Err(format!("Gagal UPDATE (HTTP {}): {}", resp.status(), resp.text().await.unwrap_or_default()))
        }
    } else {
        let resp = client.post(&printers_url).headers(headers.clone()).json(&payload).send().await
            .map_err(|e| e.to_string())?;

        if online {
            let heartbeat_url = format!("{}/heartbeat", printers_url.trim_end_matches('/'));
            let hb_payload = json!({ "local_printer": printer.address });
            let _ = client.post(&heartbeat_url).headers(headers).json(&hb_payload).send().await;
        }

        if resp.status().is_success() {
            if let Ok(res_json) = resp.json::<serde_json::Value>().await {
                if let Some(new_id) = res_json.get("id").or_else(|| res_json.get("data").and_then(|d| d.get("id"))).and_then(|v| v.as_str()) {
                    printer.id = Some(new_id.to_string());
                }
            }
            Ok(format!("Master Printer CREATE sukses [{}] [{}]", printer.alias_name, conn_type))
        } else {
            Err(format!("Gagal CREATE (HTTP {}): {}", resp.status(), resp.text().await.unwrap_or_default()))
        }
    }
}

pub async fn sync_printer_status(
    client: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    printer: &mut PrinterDeviceConfig,
    online: bool,
) -> Result<(), String> {
    let _ = sync_single_printer(client, cfg, printer, online).await;
    Ok(())
}

async fn process_print_job(
    client: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    job: &serde_json::Value,
    printer: &PrinterDeviceConfig,
    app_handle: &AppHandle,
) {
    let print_job_id = job.get("print_job_id").and_then(|v| v.as_str()).unwrap_or("");
    let job_type = job.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Template ZPL persis sama seperti ZebraPrinterService.java (Asset & Location)
    let zpl_to_send = if job_type == "Asset" {
        let id = job.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let qr_code = format!("{{\"type\":\"Asset\",\"qr\":\"{}\"}}", id);
        let name_str = job.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let code_str = job.get("codeification").and_then(|v| v.as_str()).unwrap_or("-");
        let maint_group = job.get("maintenance_group").and_then(|v| v.as_str()).unwrap_or("-");
        let copies = job.get("copies").and_then(|v| v.as_i64()).unwrap_or(1);
        let copy_count = if copies > 0 { copies } else { 1 };

        format!(
            " CT~~CD,~CC^~CT~\r\n\
            ^XA\r\n\
            ~TA000\r\n\
            ~JSN\r\n\
            ^LT0\r\n\
            ^MNW\r\n\
            ^MTT\r\n\
            ^PON\r\n\
            ^PMN\r\n\
            ^LH0,0\r\n\
            ^JMA\r\n\
            ^PR4,4\r\n\
            ~SD15\r\n\
            ^JUS\r\n\
            ^LRN\r\n\
            ^CI27\r\n\
            ^PA0,1,1,0\r\n\
            ^XZ\r\n\
            ^XA\r\n\
            ^MMT\r\n\
            ^PW400\r\n\
            ^LL180\r\n\
            ^LS0\r\n\
            ^FO16,24^BQN,2,3\r\n\
            ^FH\\^FDLA,{}^FS\r\n\
            ^FO140,22^A0N,22,20^FB250,2,2,L,0^FH\\^CI28^FD{}^FS^CI27\r\n\
            ^FO140,72^A0N,15,14^FH\\^CI28^FDCode:^FS^CI27\r\n\
            ^FO140,90^A0N,18,16^FH\\^CI28^FD{}^FS^CI27\r\n\
            ^FO140,120^A0N,15,14^FH\\^CI28^FDMaint. Group:^FS^CI27\r\n\
            ^FO140,138^A0N,18,16^FH\\^CI28^FD{}^FS^CI27\r\n\
            ^PQ{},0,1,Y\r\n\
            ^XZ\r\n",
            qr_code, name_str, code_str, maint_group, copy_count
        )
    } else if job_type == "Location" {
        let mut id = job.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            if let Some(qr_obj) = job.get("qr") {
                id = qr_obj.get("qr").and_then(|v| v.as_str()).unwrap_or("");
            }
        }
        let qr_code = format!("{{\"type\":\"Location\",\"qr\":\"{}\"}}", id);
        let name_str = job.get("name").and_then(|v| v.as_str()).unwrap_or("-");

        format!(
            " CT~~CD,~CC^~CT~\r\n\
            ^XA\r\n\
            ~TA000\r\n\
            ~JSN\r\n\
            ^LT0\r\n\
            ^MNW\r\n\
            ^MTT\r\n\
            ^PON\r\n\
            ^PMN\r\n\
            ^LH0,0\r\n\
            ^JMA\r\n\
            ^PR4,4\r\n\
            ~SD15\r\n\
            ^JUS\r\n\
            ^LRN\r\n\
            ^CI27\r\n\
            ^PA0,1,1,0\r\n\
            ^XZ\r\n\
            ^XA\r\n\
            ^MMT\r\n\
            ^PW799\r\n\
            ^LL599\r\n\
            ^LS0\r\n\
            ^FT23,98^A0N,74,74^FH\\^CI28^FDSCAN FOR QR LOCATION^FS^CI27\r\n\
            ^FT223,531^A0N,80,81^FH\\^CI28^FD{}^FS^CI27\r\n\
            ^FT275,400^BQN,2,7\r\n\
            ^FH\\^FDLA,{}^FS\r\n\
            ^PQ1,0,1,Y\r\n\
            ^XZ\r\n",
            name_str, qr_code
        )
    } else {
        // Fallback default
        format!(
            "^XA^PW600^LL400^FO50,50^A0N,36,36^FDWareify Print: {}^FS^FO50,110^A0N,28,28^FDJob ID: {}^FS^FO50,170^BQN,2,6^FDQA,{}^FS^XZ",
            job_type, print_job_id, print_job_id
        )
    };

    let res = if printer.address.starts_with("tcp://") {
        let parts: Vec<&str> = printer.address.trim_start_matches("tcp://").split(':').collect();
        let host = parts.first().unwrap_or(&"127.0.0.1");
        let port = parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(9100);
        send_zpl_to_tcp(host, port, &zpl_to_send)
    } else {
        // Windows Spooler direct print
        Ok(())
    };

    let update_url = build_full_url(&cfg.api_base_url, &cfg.path_update_job);
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(val) = HeaderValue::from_str(&cfg.api_token) {
        headers.insert("X-API-KEY", val);
    }

    if res.is_ok() {
        let _ = app_handle.emit("log-message", format!("Sukses mencetak label {} ID: {}", job_type, print_job_id));
        let status_payload = json!({
            "id": print_job_id,
            "status": "Done Printing"
        });
        let _ = client.post(&update_url).headers(headers).json(&status_payload).send().await;
    } else {
        let err_msg = res.unwrap_err();
        let _ = app_handle.emit("log-message", format!("Error cetak job: {}", err_msg));
        let status_payload = json!({
            "id": print_job_id,
            "status": "Failed",
            "error_message": err_msg
        });
        let _ = client.post(&update_url).headers(headers).json(&status_payload).send().await;
    }
}

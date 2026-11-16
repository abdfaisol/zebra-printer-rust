#[cfg(target_os = "windows")]
use std::env;
#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

const APP_NAME: &str = "PrintWareify";
const REG_RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Mengaktifkan atau menonaktifkan startup on boot di Registry Windows (HKCU\Software\Microsoft\Windows\CurrentVersion\Run)
#[cfg(target_os = "windows")]
pub fn sync_startup_registry(enable: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if enable {
        let (key, _) = hkcu
            .create_subkey(REG_RUN_SUBKEY)
            .map_err(|e| format!("Gagal membuka registry Run: {}", e))?;

        let exe_path = env::current_exe().map_err(|e| format!("Gagal mengambil path exe: {}", e))?;
        let exe_str = exe_path.to_string_lossy().to_string();

        // Tambahkan argumen --minimized agar saat boot langsung sembunyi ke tray
        let run_cmd = format!("\"{}\" --minimized", exe_str);

        key.set_value(APP_NAME, &run_cmd)
            .map_err(|e| format!("Gagal menyimpan startup registry: {}", e))?;
    } else {
        if let Ok(key) = hkcu.open_subkey_with_flags(REG_RUN_SUBKEY, KEY_SET_VALUE) {
            let _ = key.delete_value(APP_NAME);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn sync_startup_registry(_enable: bool) -> Result<(), String> {
    Ok(())
}

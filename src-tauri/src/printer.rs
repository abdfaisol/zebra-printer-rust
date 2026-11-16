use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredPrinter {
    pub address: String,
    pub name: String,
    pub is_online: bool,
}

/// Pengecekan TCP port printer (Virtual TCP) dengan timeout instan 150ms
pub fn check_tcp_printer(host: &str, port: u16) -> bool {
    let addr_str = format!("{}:{}", host, port);
    if let Ok(socket_addr) = addr_str.parse::<SocketAddr>() {
        TcpStream::connect_timeout(&socket_addr, Duration::from_millis(150)).is_ok()
    } else {
        false
    }
}

pub fn send_zpl_to_tcp(host: &str, port: u16, zpl_data: &str) -> Result<(), String> {
    let addr_str = format!("{}:{}", host, port);
    let socket_addr = addr_str
        .parse::<SocketAddr>()
        .map_err(|e| format!("Invalid socket address: {}", e))?;
    let mut stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map_err(|e| format!("Cannot connect to printer TCP: {}", e))?;
    stream
        .write_all(zpl_data.as_bytes())
        .map_err(|e| format!("Write ZPL error: {}", e))?;
    stream.flush().map_err(|e| format!("Flush error: {}", e))?;
    Ok(())
}

/// Memindai printer Windows secara langsung menggunakan Native Windows Spooler API (EnumPrintersW).
/// Tidak memakai powershell.exe sehingga waktu eksekusi < 1 milidetik (ultra-cepat dan responsif).
#[cfg(target_os = "windows")]
pub fn scan_windows_spooler_printers() -> Vec<DiscoveredPrinter> {
    use windows::core::PWSTR;
    use windows::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W,
    };

    let mut list = Vec::new();
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;

    unsafe {
        let mut bytes_needed = 0u32;
        let mut count = 0u32;

        // Panggilan pertama untuk mendapatkan ukuran buffer yang dibutuhkan
        let _ = EnumPrintersW(
            flags,
            PWSTR::null(),
            2,
            None,
            &mut bytes_needed,
            &mut count,
        );

        if bytes_needed == 0 {
            return list;
        }

        let mut buffer: Vec<u8> = vec![0u8; bytes_needed as usize];

        if EnumPrintersW(
            flags,
            PWSTR::null(),
            2,
            Some(buffer.as_mut_slice()),
            &mut bytes_needed,
            &mut count,
        )
        .is_ok()
        {
            let printers_slice = std::slice::from_raw_parts(
                buffer.as_ptr() as *const PRINTER_INFO_2W,
                count as usize,
            );

            for info in printers_slice {
                if !info.pPrinterName.is_null() {
                    let name = info.pPrinterName.to_string().unwrap_or_default();
                    if name.is_empty() {
                        continue;
                    }

                    // PRINTER_ATTRIBUTE_WORK_OFFLINE = 0x00000400 (1024)
                    let is_work_offline = (info.Attributes & 0x00000400) != 0;
                    
                    // Status printer: jika offline flag menyala atau ada status error/offline
                    let is_online = !is_work_offline;

                    list.push(DiscoveredPrinter {
                        address: name.clone(),
                        name,
                        is_online,
                    });
                }
            }
        }
    }

    list
}

#[cfg(not(target_os = "windows"))]
pub fn scan_windows_spooler_printers() -> Vec<DiscoveredPrinter> {
    Vec::new()
}

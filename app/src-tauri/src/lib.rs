mod hub;

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};

static HUB_STARTED: AtomicBool = AtomicBool::new(false);

/// Called by the assistant window that hosts the hub (usually PC 1).
/// Starts the hub once and returns "ip:port" so the other PCs know where to connect.
#[tauri::command]
async fn start_hub() -> Result<String, String> {
    if !HUB_STARTED.swap(true, Ordering::SeqCst) {
        if let Err(e) = hub::start().await {
            HUB_STARTED.store(false, Ordering::SeqCst);
            return Err(e);
        }
    }
    Ok(format!("{}:{}", local_ip(), hub::PORT))
}

/// Best guess at this PC's address on the lab network.
/// "Connecting" a UDP socket sends no packets; it only makes Windows pick
/// the network card it would use, so we can read that card's address.
fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("10.255.255.255:1")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start_hub])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod discovery;
mod hub;

use std::net::{Ipv4Addr, UdpSocket};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Passcode of the hub running in this app, if this PC hosts it.
static HUB_PASSCODE: Mutex<Option<String>> = Mutex::const_new(None);

#[derive(Serialize)]
struct HubInfo {
    /// "ip:port" the other PCs connect to.
    address: String,
    /// Session passcode the back assistant must type.
    passcode: String,
}

/// Called by the assistant window that hosts the hub (usually PC 1).
/// Starts the hub once and returns where it is and its passcode.
#[tauri::command]
async fn start_hub() -> Result<HubInfo, String> {
    let mut saved = HUB_PASSCODE.lock().await;
    if saved.is_none() {
        *saved = Some(hub::start().await?);
        // Let student PCs find this hub without typing its address. If the UDP
        // port is busy, the hub still works: students can type the address.
        let hub_id = discovery::new_hub_id()?;
        if let Err(e) = discovery::start_responder(hub_id, discovery::DISCOVERY_PORT).await {
            eprintln!("hub discovery is off: {e}");
        }
    }
    Ok(HubInfo {
        address: format!("{}:{}", local_ip(), hub::PORT),
        passcode: saved.clone().unwrap_or_default(),
    })
}

/// Looks for hubs on the local network for about 1.5 seconds.
/// Returns one address per hub that answered (usually one).
#[tauri::command]
async fn find_hubs() -> Result<Vec<String>, String> {
    let my_ip = local_ip().parse::<Ipv4Addr>().ok();
    let targets = discovery::targets(discovery::DISCOVERY_PORT, my_ip);
    discovery::find_hubs_on(&targets, discovery::SEARCH_TIME).await
}

/// Lab settings installed by the lab admins, so students cannot pick a role.
/// File: C:\ProgramData\Mercusoar\config.json
///   student PCs: { "role": "student", "pc": 7, "hub_address": "192.168.1.10" }
///   PC 1:        { "role": "assistant", "seat": "Front" }
///   PC 40:       { "role": "assistant", "seat": "Back", "hub_address": "192.168.1.10" }
/// "pc" is optional: without it the student types their PC number.
/// "hub_address" is optional for students: without it the app finds the hub
/// on the network by itself. With it, the app never searches, which is safer
/// in a real lab (nobody can pretend to be the hub).
#[derive(Serialize, Deserialize)]
struct LabConfig {
    role: String,
    seat: Option<String>,
    pc: Option<u32>,
    hub_address: Option<String>,
}

/// Returns the lab config, or None when there is no file (developer laptops),
/// in which case the app shows the role picker for testing.
#[tauri::command]
fn load_config() -> Result<Option<LabConfig>, String> {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    let path = std::path::Path::new(&base).join("Mercusoar").join("config.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(None) };
    parse_config(&text).map(Some).map_err(|e| format!("{}: {e}", path.display()))
}

/// Check the config file's contents. Kept separate so it can be tested.
fn parse_config(text: &str) -> Result<LabConfig, String> {
    let config: LabConfig = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    match (config.role.as_str(), config.seat.as_deref()) {
        ("student", _) | ("assistant", Some("Front" | "Back")) => {}
        _ => return Err("role must be \"student\", or \"assistant\" with seat \"Front\" or \"Back\"".into()),
    }
    if let Some(pc) = config.pc {
        if config.role != "student" || !(2..=39).contains(&pc) {
            return Err("\"pc\" is only for students and must be 2 to 39".into());
        }
    }
    if let Some(address) = &config.hub_address {
        if address != "localhost" && address.parse::<std::net::Ipv4Addr>().is_err() {
            return Err("\"hub_address\" must be an IPv4 address like 192.168.1.10".into());
        }
    }
    Ok(config)
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
        .invoke_handler(tauri::generate_handler![start_hub, load_config, find_hubs])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::parse_config;

    #[test]
    fn accepts_student_and_both_assistant_seats() {
        assert!(parse_config(r#"{"role":"student","hub_address":"192.168.1.10"}"#).is_ok());
        assert!(parse_config(r#"{"role":"assistant","seat":"Front"}"#).is_ok());
        assert!(parse_config(r#"{"role":"assistant","seat":"Back","hub_address":"192.168.1.10"}"#).is_ok());
        assert!(parse_config(r#"{"role":"student","pc":7,"hub_address":"192.168.1.10"}"#).is_ok());
    }

    #[test]
    fn rejects_bad_configs() {
        assert!(parse_config(r#"{"role":"assistant"}"#).is_err()); // no seat
        assert!(parse_config(r#"{"role":"assistant","seat":"Middle"}"#).is_err());
        assert!(parse_config(r#"{"role":"admin"}"#).is_err());
        assert!(parse_config("not json").is_err());
        assert!(parse_config(r#"{"role":"student","pc":1}"#).is_err()); // admin PC
        assert!(parse_config(r#"{"role":"student","pc":40}"#).is_err());
        assert!(parse_config(r#"{"role":"assistant","seat":"Front","pc":7}"#).is_err());
        assert!(parse_config(r#"{"role":"student","hub_address":"evil.example/x"}"#).is_err());
        assert!(parse_config(r#"{"role":"student","hub_address":"192.168.1.300"}"#).is_err());
    }
}

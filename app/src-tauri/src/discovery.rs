//! Finding the hub automatically, so nobody has to type its IP address.
//!
//! The student app asks the whole local network one small UDP question,
//! "is there a Mercusoar hub?", and every hub answers. The answer's sender
//! address is the hub's IP.
//!
//! Only the hub listens for UDP. The students only ask, and Windows lets the
//! answer to their broadcast in by default, so student PCs need no new
//! firewall rule.
//!
//! Every hub run gets a random `hub_id`. If the same hub answers twice (on the
//! hub laptop itself it answers both on 127.0.0.1 and on its Wi-Fi address),
//! it is counted once. If DIFFERENT hubs answer, the app does not join
//! automatically: one of them could be a fake hub run by a classmate.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout, Instant};

/// UDP port the hub listens on for the question. Every PC uses the same one.
pub const DISCOVERY_PORT: u16 = 47801;

/// The question a student app broadcasts.
const REQUEST: &[u8] = b"MERCUSOAR_DISCOVER_V1";
/// The hub's answer starts with this, followed by its 6-character hub_id.
const REPLY_PREFIX: &[u8] = b"MERCUSOAR_HUB ";
const HUB_ID_LEN: usize = 6;

/// How often the question is sent during one search, and how long one search lasts.
const ASK_EVERY: Duration = Duration::from_millis(500);
pub const SEARCH_TIME: Duration = Duration::from_millis(1500);

/// A random id for this hub run, e.g. "3fa9c1".
pub fn new_hub_id() -> Result<String, String> {
    let mut bytes = [0u8; HUB_ID_LEN / 2];
    getrandom::fill(&mut bytes).map_err(|e| format!("no secure randomness: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Answer the question on `port` (0 = any free port, for tests) in the
/// background. Returns the port actually used.
pub async fn start_responder(hub_id: String, port: u16) -> Result<u16, String> {
    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("could not open UDP port {port}: {e}"))?;
    let port = socket.local_addr().map_err(|e| e.to_string())?.port();

    let mut reply = REPLY_PREFIX.to_vec();
    reply.extend_from_slice(hub_id.as_bytes());
    // The answer is never bigger than the question, so nobody can use the hub
    // to flood a third computer with more data than they send themselves.
    debug_assert!(reply.len() <= REQUEST.len());

    tauri::async_runtime::spawn(async move {
        let mut buf = [0u8; 64];
        loop {
            match socket.recv_from(&mut buf).await {
                // Only the exact question gets an answer. Everything else is ignored.
                Ok((n, from)) if &buf[..n] == REQUEST => {
                    let _ = socket.send_to(&reply, from).await;
                }
                Ok(_) => {}
                // Windows reports "connection reset" on UDP when an earlier
                // answer could not be delivered. Not fatal: keep listening.
                Err(_) => sleep(Duration::from_millis(10)).await,
            }
        }
    });
    Ok(port)
}

/// Ask `targets` (broadcast addresses and 127.0.0.1) for hubs for `wait`.
/// Returns one IP address per hub that answered.
pub async fn find_hubs_on(targets: &[SocketAddr], wait: Duration) -> Result<Vec<String>, String> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await.map_err(|e| e.to_string())?;
    socket.set_broadcast(true).map_err(|e| e.to_string())?;

    let mut hubs: HashMap<String, IpAddr> = HashMap::new();
    let deadline = Instant::now() + wait;
    let mut next_ask = Instant::now();
    let mut buf = [0u8; 64];

    while Instant::now() < deadline {
        if Instant::now() >= next_ask {
            for target in targets {
                // A target can be unreachable (e.g. no Wi-Fi). Try the others.
                let _ = socket.send_to(REQUEST, target).await;
            }
            next_ask += ASK_EVERY;
        }
        let until = next_ask.min(deadline);
        match timeout(until.saturating_duration_since(Instant::now()), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => add_reply(&mut hubs, from.ip(), &buf[..n]),
            // "Connection reset": 127.0.0.1 has no hub. Keep listening.
            Ok(Err(_)) => {}
            Err(_) => {} // time to ask again, or the search is over
        }
    }

    let mut found: Vec<String> = hubs.into_values().map(|ip| ip.to_string()).collect();
    found.sort();
    Ok(found)
}

/// Remember one answer. The same hub can answer from two addresses on the hub
/// laptop itself; 127.0.0.1 wins, because the hub treats its own laptop specially.
fn add_reply(hubs: &mut HashMap<String, IpAddr>, from: IpAddr, data: &[u8]) {
    let Some(id) = data.strip_prefix(REPLY_PREFIX) else { return };
    if id.len() != HUB_ID_LEN || !id.iter().all(u8::is_ascii_hexdigit) {
        return;
    }
    let id = String::from_utf8_lossy(id).into_owned();
    match hubs.get(&id) {
        Some(known) if known.is_loopback() => {}
        Some(_) if !from.is_loopback() => {}
        _ => {
            hubs.insert(id, from);
        }
    }
}

/// Where the student app sends the question: everyone on the network, the
/// network of this PC's Wi-Fi card (Windows may send the first one out of
/// another card, e.g. a virtual one), and this PC itself.
pub fn targets(port: u16, local_ip: Option<Ipv4Addr>) -> Vec<SocketAddr> {
    let mut list = vec![
        SocketAddr::from((Ipv4Addr::BROADCAST, port)),
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
    ];
    if let Some(ip) = local_ip.filter(|ip| !ip.is_loopback()) {
        let [a, b, c, _] = ip.octets();
        // Phone hotspots and home Wi-Fi use a /24 network: x.y.z.255 reaches everyone.
        list.push(SocketAddr::from((Ipv4Addr::new(a, b, c, 255), port)));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(port: u16) -> Vec<SocketAddr> {
        vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))]
    }

    #[test]
    fn hub_ids_are_six_hex_characters_and_differ() {
        let a = new_hub_id().unwrap();
        let b = new_hub_id().unwrap();
        assert_eq!(a.len(), HUB_ID_LEN);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn answer_is_not_bigger_than_question() {
        assert!(REPLY_PREFIX.len() + HUB_ID_LEN <= REQUEST.len());
    }

    #[test]
    fn same_hub_twice_counts_once_and_prefers_loopback() {
        let mut hubs = HashMap::new();
        let lan: IpAddr = "192.168.43.120".parse().unwrap();
        let me: IpAddr = "127.0.0.1".parse().unwrap();
        add_reply(&mut hubs, lan, b"MERCUSOAR_HUB a1b2c3");
        add_reply(&mut hubs, me, b"MERCUSOAR_HUB a1b2c3");
        add_reply(&mut hubs, lan, b"MERCUSOAR_HUB a1b2c3");
        assert_eq!(hubs.len(), 1);
        assert_eq!(hubs["a1b2c3"], me);
    }

    #[test]
    fn different_hubs_are_all_reported_and_junk_is_ignored() {
        let mut hubs = HashMap::new();
        add_reply(&mut hubs, "192.168.43.120".parse().unwrap(), b"MERCUSOAR_HUB a1b2c3");
        add_reply(&mut hubs, "192.168.43.77".parse().unwrap(), b"MERCUSOAR_HUB 999999");
        add_reply(&mut hubs, "192.168.43.5".parse().unwrap(), b"hello");
        add_reply(&mut hubs, "192.168.43.5".parse().unwrap(), b"MERCUSOAR_HUB zzzzzz");
        add_reply(&mut hubs, "192.168.43.5".parse().unwrap(), b"MERCUSOAR_HUB a1b2c3d4");
        assert_eq!(hubs.len(), 2);
    }

    #[test]
    fn targets_include_the_wifi_network() {
        let list = targets(47801, Some(Ipv4Addr::new(192, 168, 43, 120)));
        assert!(list.contains(&"192.168.43.255:47801".parse().unwrap()));
        assert!(list.contains(&"255.255.255.255:47801".parse().unwrap()));
        assert!(list.contains(&"127.0.0.1:47801".parse().unwrap()));
        assert_eq!(targets(47801, None).len(), 2);
    }

    #[tokio::test]
    async fn finds_a_running_hub() {
        let port = start_responder(new_hub_id().unwrap(), 0).await.unwrap();
        let found = find_hubs_on(&local(port), Duration::from_millis(600)).await.unwrap();
        assert_eq!(found, vec!["127.0.0.1".to_string()]);
    }

    #[tokio::test]
    async fn finds_nothing_when_no_hub_runs() {
        // Grab a free port, then close it, so nothing listens there.
        let port = UdpSocket::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let found = find_hubs_on(&local(port), Duration::from_millis(400)).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn reports_two_hubs_when_two_answer() {
        let p1 = start_responder(new_hub_id().unwrap(), 0).await.unwrap();
        let p2 = start_responder(new_hub_id().unwrap(), 0).await.unwrap();
        let targets = [local(p1), local(p2)].concat();
        let found = find_hubs_on(&targets, Duration::from_millis(600)).await.unwrap();
        assert_eq!(found.len(), 2);
    }

    #[tokio::test]
    async fn ignores_anything_but_the_question() {
        let port = start_responder(new_hub_id().unwrap(), 0).await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket.send_to(b"MERCUSOAR_DISCOVER_V2", ("127.0.0.1", port)).await.unwrap();
        let mut buf = [0u8; 64];
        let got = timeout(Duration::from_millis(300), socket.recv_from(&mut buf)).await;
        assert!(got.is_err(), "the hub must not answer an unknown message");
    }
}

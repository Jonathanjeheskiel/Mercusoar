//! Mercusoar hub
//!
//! The hub is a small WebSocket server that runs on ONE admin PC (usually PC 1).
//! Every Mercusoar window in the lab connects to it:
//!   - student PCs send Flares (help requests),
//!   - both lab assistants receive the shared queue and claim / resolve Flares.
//!
//! The hub keeps the queue in memory on the admin PC, because student PCs are
//! wiped after every session.
//!
//! Privacy rule: students NEVER receive information about other students.
//! Only assistants see PC numbers.
//!
//! Messages are JSON objects with a "type" field. See `ClientMsg` for what clients
//! can send, and `broadcast()` for what the hub sends back.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

/// TCP port the hub listens on. Every PC must use the same port.
pub const PORT: u16 = 47800;

/// Longest question a student can type (characters).
const MAX_QUESTION_LEN: usize = 200;

// ----- Limits, so one misbehaving computer cannot take the hub down -----
/// Largest message a client may send. Real messages are well under 1 KB.
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
/// Open connections the hub accepts in total, and from one computer.
/// The hub PC itself (127.0.0.1) has no per-computer limit, for the demo script.
const MAX_CONNECTIONS: usize = 200;
const MAX_CONNECTIONS_PER_IP: usize = 4;
/// Time a new connection gets to finish the WebSocket handshake and say hello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Messages per second one window may send before it is disconnected.
const MAX_MESSAGES_PER_SEC: u32 = 20;
/// Messages waiting to be written to one window. A window that falls this far
/// behind (stopped reading) is disconnected and reconnects with fresh state.
const OUTBOX_SIZE: usize = 64;

/// Student PCs are 2-39. PC 1 (front) and PC 40 (back) are admin PCs.
/// Must match FIRST_STUDENT_PC / LAST_STUDENT_PC in src/types.ts.
const STUDENT_PCS: std::ops::RangeInclusive<u32> = 2..=39;

/// Wrong passcodes allowed from one computer before it is locked out.
/// The 1 s delay per wrong guess alone does not stop guessing, because a
/// script can open many connections at once.
const MAX_FAILED_AUTH: u32 = 5;
/// How long a computer stays locked out after too many wrong passcodes.
const AUTH_LOCKOUT: Duration = Duration::from_secs(10 * 60);

/// One help request from one student PC.
#[derive(Clone, Serialize)]
struct Flare {
    id: u64,
    pc: u32,
    question: String,
    created_ms: u64,
    /// Name of the assistant who clicked "I'll take it", e.g. "Front".
    claimed_by: Option<String>,
    /// Computer that sent it. Never sent to anyone.
    #[serde(skip)]
    ip: IpAddr,
}

/// Everything a client is allowed to send to the hub.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    // Sent once right after connecting, to say who this window is.
    HelloStudent { pc: u32 },
    // Assistants must also send the session passcode shown on the hub PC.
    HelloAssistant { name: String, passcode: String },
    // Student actions
    Flare { question: String },
    Cancel,
    // Assistant actions
    Claim { id: u64 },
    Release { id: u64 },
    Resolve { id: u64 },
}

enum Role {
    Unknown,
    Student(u32),
    Assistant(String),
}

struct Client {
    role: Role,
    /// Address of the computer this window runs on.
    ip: IpAddr,
    /// Messages pushed here are written to this client's WebSocket.
    tx: mpsc::Sender<Message>,
    /// Dropped together with the client, which tells its connection to close.
    _kick: oneshot::Sender<()>,
}

impl Client {
    /// Queue a message for this window. False if its outbox is full or closed.
    fn push(&self, text: String) -> bool {
        self.tx.try_send(Message::text(text)).is_ok()
    }
}

struct Hub {
    /// Secret an assistant must send to be treated as an assistant.
    /// Without it a tampered student app sees nothing and controls nothing.
    passcode: String,
    next_client_id: u64,
    next_flare_id: u64,
    clients: HashMap<u64, Client>,
    /// Wrong passcode attempts per computer: (count, time of the first one).
    failed_auth: HashMap<IpAddr, (u32, Instant)>,
    /// Open Flares, oldest first. This order IS the fair queue.
    flares: Vec<Flare>,
}

type SharedHub = Arc<Mutex<Hub>>;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Compare passcodes the way people type them: ignore case, spaces and the dash,
/// so "k7q4mz" and "K7Q-4MZ" both match.
fn normalize_passcode(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// The two admin seats. Anything else in `hello_assistant` is rejected.
fn is_valid_seat(name: &str) -> bool {
    matches!(name, "Front" | "Back")
}

/// Make a short random passcode like "K7Q-4MZ".
/// Uses letters and digits that are hard to confuse (no 0/O, 1/I/L).
/// Randomness comes from the operating system's secure random generator.
fn new_passcode() -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    // Bytes at or above this limit are skipped, so every letter is equally likely.
    let limit = (256 / ALPHABET.len() * ALPHABET.len()) as u8;
    let mut out = String::new();
    let mut picked = 0;
    while picked < 6 {
        let mut buf = [0u8; 16];
        getrandom::fill(&mut buf).map_err(|e| format!("no secure randomness: {e}"))?;
        for b in buf.into_iter().filter(|&b| b < limit) {
            if picked == 6 {
                break;
            }
            if picked == 3 {
                out.push('-');
            }
            out.push(ALPHABET[b as usize % ALPHABET.len()] as char);
            picked += 1;
        }
    }
    Ok(out)
}

impl Hub {
    fn new(passcode: String) -> Self {
        Hub {
            passcode,
            next_client_id: 0,
            next_flare_id: 0,
            clients: HashMap::new(),
            failed_auth: HashMap::new(),
            flares: Vec::new(),
        }
    }

    /// Apply one message from client `cid` to the hub state.
    /// Returns false if the connection should be closed (wrong passcode).
    fn handle(&mut self, cid: u64, msg: ClientMsg) -> bool {
        match msg {
            ClientMsg::HelloStudent { pc } => {
                let Some(c) = self.clients.get(&cid) else { return false };
                // A window picks its role once; it cannot switch later.
                if !matches!(c.role, Role::Unknown) {
                    return true;
                }
                let ip = c.ip;
                if !STUDENT_PCS.contains(&pc) {
                    self.reject(cid, "That is not a student PC number.");
                    return false;
                }
                if let Some(reason) = self.pc_taken(pc, ip) {
                    self.reject(cid, &reason);
                    return false;
                }
                let Some(c) = self.clients.get_mut(&cid) else { return false };
                c.role = Role::Student(pc);
            }
            ClientMsg::HelloAssistant { name, passcode } => {
                let Some(c) = self.clients.get(&cid) else { return false };
                if !matches!(c.role, Role::Unknown) {
                    return true;
                }
                let ip = c.ip;
                if self.is_locked_out(ip) {
                    self.reject(cid, "Too many wrong passcodes from this PC. Try again in 10 minutes.");
                    return false;
                }
                if !is_valid_seat(&name) || normalize_passcode(&passcode) != normalize_passcode(&self.passcode) {
                    self.record_failed_auth(ip);
                    self.reject(cid, "Wrong passcode");
                    return false;
                }
                self.failed_auth.remove(&ip);
                let Some(c) = self.clients.get_mut(&cid) else { return false };
                c.role = Role::Assistant(name);
                c.push(json!({ "type": "auth_ok" }).to_string());
            }
            ClientMsg::Flare { question } => {
                let Some(pc) = self.student_pc(cid) else { return true };
                let Some(ip) = self.clients.get(&cid).map(|c| c.ip) else { return true };
                let question: String = question.trim().chars().take(MAX_QUESTION_LEN).collect();
                // One open Flare per PC: sending again just updates the question
                // and keeps the student's place in the queue.
                if let Some(f) = self.flares.iter_mut().find(|f| f.pc == pc) {
                    f.question = question;
                } else {
                    self.next_flare_id += 1;
                    self.flares.push(Flare {
                        id: self.next_flare_id,
                        pc,
                        question,
                        created_ms: now_ms(),
                        claimed_by: None,
                        ip,
                    });
                }
            }
            ClientMsg::Cancel => {
                if let Some(pc) = self.student_pc(cid) {
                    self.flares.retain(|f| f.pc != pc);
                }
            }
            ClientMsg::Claim { id } => {
                let Some(name) = self.assistant_name(cid) else { return true };
                // Only claim if nobody else has it yet, so two assistants
                // never walk to the same student.
                if let Some(f) = self.flares.iter_mut().find(|f| f.id == id) {
                    if f.claimed_by.is_none() {
                        f.claimed_by = Some(name);
                    }
                }
            }
            ClientMsg::Release { id } => {
                if self.assistant_name(cid).is_none() {
                    return true;
                }
                if let Some(f) = self.flares.iter_mut().find(|f| f.id == id) {
                    f.claimed_by = None;
                }
            }
            ClientMsg::Resolve { id } => {
                if self.assistant_name(cid).is_none() {
                    return true;
                }
                if let Some(pos) = self.flares.iter().position(|f| f.id == id) {
                    let pc = self.flares.remove(pos).pc;
                    // Tell that student their question was answered.
                    self.send_to_pc(pc, json!({ "type": "resolved" }));
                }
            }
        }
        true
    }

    /// Tell a client why it is being disconnected.
    fn reject(&self, cid: u64, message: &str) {
        if let Some(c) = self.clients.get(&cid) {
            c.push(json!({ "type": "auth_error", "message": message }).to_string());
        }
    }

    fn is_locked_out(&mut self, ip: IpAddr) -> bool {
        match self.failed_auth.get(&ip) {
            Some(&(_, first)) if first.elapsed() >= AUTH_LOCKOUT => {
                self.failed_auth.remove(&ip);
                false
            }
            Some(&(count, _)) => count >= MAX_FAILED_AUTH,
            None => false,
        }
    }

    fn record_failed_auth(&mut self, ip: IpAddr) {
        self.failed_auth.entry(ip).or_insert((0, Instant::now())).0 += 1;
    }

    /// Stops one student from posing as another PC (to read or cancel that
    /// PC's Flare). While a PC number is online or has an open Flare, it
    /// belongs to the computer that used it, and that computer cannot use a
    /// second PC number at the same time. Typing the wrong number by mistake
    /// is fixed by pressing Exit and starting again.
    ///
    /// Windows on the hub PC itself (127.0.0.1) are exempt, so the demo script
    /// and single-laptop testing keep working.
    fn pc_taken(&self, pc: u32, ip: IpAddr) -> Option<String> {
        if ip.is_loopback() {
            return None;
        }
        let students = self.clients.values().filter_map(|c| match c.role {
            Role::Student(p) => Some((p, c.ip)),
            _ => None,
        });
        let flares = self.flares.iter().map(|f| (f.pc, f.ip));
        for (p, owner) in students.chain(flares) {
            if p == pc && owner != ip {
                return Some(format!("PC {pc} is already in use by another computer. Check your PC number."));
            }
            if p != pc && owner == ip {
                return Some(format!("This computer is already signed in as PC {p}."));
            }
        }
        None
    }

    fn student_pc(&self, cid: u64) -> Option<u32> {
        match self.clients.get(&cid)?.role {
            Role::Student(pc) => Some(pc),
            _ => None,
        }
    }

    fn assistant_name(&self, cid: u64) -> Option<String> {
        match &self.clients.get(&cid)?.role {
            Role::Assistant(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn send_to_pc(&self, pc: u32, value: serde_json::Value) {
        let text = value.to_string();
        for c in self.clients.values() {
            if matches!(c.role, Role::Student(p) if p == pc) {
                c.push(text.clone());
            }
        }
    }

    /// Send every client its own view of the current state.
    /// Windows that stopped reading are disconnected.
    fn broadcast(&mut self) {
        let now = now_ms();
        let online: Vec<u32> = self
            .clients
            .values()
            .filter_map(|c| match c.role {
                Role::Student(pc) => Some(pc),
                _ => None,
            })
            .collect();

        // Assistants see the whole queue and which PCs are online.
        let assistant_view =
            json!({ "type": "state", "now_ms": now, "flares": self.flares, "online": online })
                .to_string();

        let mut stuck = Vec::new();
        for (&cid, c) in &self.clients {
            let sent = match &c.role {
                Role::Assistant(_) => c.push(assistant_view.clone()),
                Role::Student(pc) => {
                    // Students only see their OWN Flare and their place in line.
                    let mine = self.flares.iter().position(|f| f.pc == *pc).map(|i| {
                        let f = &self.flares[i];
                        json!({
                            "question": f.question,
                            "position": i + 1,
                            "claimed_by": f.claimed_by,
                        })
                    });
                    let view = json!({ "type": "my_flare", "flare": mine });
                    c.push(view.to_string())
                }
                Role::Unknown => true,
            };
            if !sent {
                stuck.push(cid);
            }
        }
        for cid in stuck {
            self.clients.remove(&cid);
        }
    }
}

/// Start listening on every network interface. Returns the session passcode
/// once the port is open, then keeps accepting connections in the background.
pub async fn start() -> Result<String, String> {
    let listener = TcpListener::bind(("0.0.0.0", PORT))
        .await
        .map_err(|e| format!("could not open port {PORT}: {e}"))?;

    let passcode = new_passcode()?;
    let hub: SharedHub = Arc::new(Mutex::new(Hub::new(passcode.clone())));
    let limits = Arc::new(ConnLimits::default());
    tauri::async_runtime::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    // Over the limit: drop the connection right away.
                    let Some(slot) = limits.try_take(addr.ip()) else { continue };
                    tauri::async_runtime::spawn(handle_connection(stream, addr, hub.clone(), slot));
                }
                // e.g. out of sockets: back off instead of spinning at 100% CPU.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    });
    Ok(passcode)
}

/// Counts open connections, in total and per computer.
#[derive(Default)]
struct ConnLimits {
    open: std::sync::Mutex<(usize, HashMap<IpAddr, usize>)>,
}

/// One open connection. Frees its place when dropped.
struct ConnSlot {
    limits: Arc<ConnLimits>,
    ip: IpAddr,
}

impl ConnLimits {
    fn try_take(self: &Arc<Self>, ip: IpAddr) -> Option<ConnSlot> {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        let (total, per_ip) = &mut *open;
        let mine = per_ip.get(&ip).copied().unwrap_or(0);
        if *total >= MAX_CONNECTIONS || (!ip.is_loopback() && mine >= MAX_CONNECTIONS_PER_IP) {
            return None;
        }
        *total += 1;
        per_ip.insert(ip, mine + 1);
        Some(ConnSlot { limits: self.clone(), ip })
    }
}

impl Drop for ConnSlot {
    fn drop(&mut self) {
        let mut open = self.limits.open.lock().unwrap_or_else(|e| e.into_inner());
        let (total, per_ip) = &mut *open;
        *total -= 1;
        if let Some(n) = per_ip.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                per_ip.remove(&self.ip);
            }
        }
    }
}

/// Browsers attach an Origin header to WebSocket connections. Without this
/// check, any web page opened on a lab PC (or on the hub PC) could connect to
/// the hub in the background and act as a student. Only the Mercusoar window
/// is allowed. Clients without an Origin header (not a browser, e.g. the demo
/// script) are let through; they still have to follow every hub rule.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    // Mercusoar's own window: Windows uses http://tauri.localhost,
    // macOS and Linux use tauri://localhost.
    if matches!(origin, "http://tauri.localhost" | "https://tauri.localhost" | "tauri://localhost") {
        return true;
    }
    // `npm run tauri dev` serves the window from the Vite dev server.
    cfg!(debug_assertions) && origin.starts_with("http://") && origin.ends_with(":1420")
}

/// Serve one connected window until it disconnects.
/// `_slot` keeps this connection counted until the function returns.
async fn handle_connection(stream: TcpStream, addr: SocketAddr, hub: SharedHub, _slot: ConnSlot) {
    let check_origin = |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
        let origin = req.headers().get("origin").and_then(|v| v.to_str().ok());
        if origin_allowed(origin) {
            Ok(res)
        } else {
            let mut deny = ErrorResponse::new(Some("origin not allowed".into()));
            *deny.status_mut() = StatusCode::FORBIDDEN;
            Err(deny)
        }
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let handshake = tokio_tungstenite::accept_hdr_async_with_config(stream, check_origin, Some(config));
    let Ok(Ok(ws)) = tokio::time::timeout(HELLO_TIMEOUT, handshake).await else { return };
    let (mut ws_out, mut ws_in) = ws.split();

    // Register the client with a channel we can push messages into.
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOX_SIZE);
    let (kick, mut kicked) = oneshot::channel::<()>();
    let cid = {
        let mut h = hub.lock().await;
        h.next_client_id += 1;
        let cid = h.next_client_id;
        h.clients.insert(cid, Client { role: Role::Unknown, ip: addr.ip(), tx, _kick: kick });
        cid
    };

    // Writer task: forward queued messages to the socket.
    let mut writer = tauri::async_runtime::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_out.send(msg).await.is_err() {
                break;
            }
        }
    });

    // A window must say who it is soon after connecting.
    let hello_deadline = tokio::time::sleep(HELLO_TIMEOUT);
    tokio::pin!(hello_deadline);
    let mut identified = false;
    let mut window_start = Instant::now();
    let mut in_window = 0u32;

    // Reader loop: apply each incoming message, then update everyone.
    loop {
        let msg = tokio::select! {
            msg = ws_in.next() => msg,
            _ = &mut hello_deadline, if !identified => break,
            _ = &mut kicked => break, // removed by the hub (stopped reading)
        };
        let Some(Ok(msg)) = msg else { break };

        if window_start.elapsed() >= Duration::from_secs(1) {
            window_start = Instant::now();
            in_window = 0;
        }
        in_window += 1;
        if in_window > MAX_MESSAGES_PER_SEC {
            break; // flooding; the app reconnects on its own if this was a glitch
        }

        let Message::Text(text) = msg else { continue };
        let Ok(parsed) = serde_json::from_str::<ClientMsg>(&text) else { continue };
        let mut h = hub.lock().await;
        let keep_open = h.handle(cid, parsed);
        identified = h.clients.get(&cid).is_some_and(|c| !matches!(c.role, Role::Unknown));
        h.broadcast();
        drop(h);
        if !keep_open {
            // Rejected (e.g. wrong passcode): wait a moment before closing, so
            // guessing passcodes one by one would take far too long.
            tokio::time::sleep(Duration::from_secs(1)).await;
            break;
        }
    }

    // Disconnected. The student's Flare (if any) stays in the queue on purpose,
    // so a crash or network blip does not lose their place.
    let mut h = hub.lock().await;
    h.clients.remove(&cid); // drops this client's sender, so the writer finishes
    h.broadcast();
    drop(h);
    // Let queued messages (e.g. "Wrong passcode") go out first, but do not
    // wait forever on a window that stopped reading.
    if tokio::time::timeout(Duration::from_secs(2), &mut writer).await.is_err() {
        writer.abort();
    }
}

// ===== Tests =====
// Run with: cargo test   (inside app/src-tauri)
// These check the hub's rules directly, without a real network.
#[cfg(test)]
mod tests {
    use super::*;

    /// Add a fake connected window and return its id plus its inbox.
    fn connect(hub: &mut Hub) -> (u64, mpsc::Receiver<Message>) {
        connect_from(hub, "127.0.0.1")
    }

    /// Same as `connect`, from a given computer address.
    fn connect_from(hub: &mut Hub, ip: &str) -> (u64, mpsc::Receiver<Message>) {
        let (tx, rx) = mpsc::channel(OUTBOX_SIZE);
        let (kick, _) = oneshot::channel();
        hub.next_client_id += 1;
        let cid = hub.next_client_id;
        hub.clients.insert(cid, Client { role: Role::Unknown, ip: ip.parse().unwrap(), tx, _kick: kick });
        (cid, rx)
    }

    /// Everything a window has received so far, as JSON values.
    fn inbox(rx: &mut mpsc::Receiver<Message>) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        while let Ok(Message::Text(t)) = rx.try_recv() {
            out.push(serde_json::from_str(&t).unwrap());
        }
        out
    }

    fn assistant_hello(passcode: &str) -> ClientMsg {
        ClientMsg::HelloAssistant { name: "Front".into(), passcode: passcode.into() }
    }

    #[test]
    fn passcode_looks_like_abc_def() {
        let p = new_passcode().unwrap();
        assert_eq!(p.len(), 7);
        assert_eq!(&p[3..4], "-");
        assert_ne!(p, new_passcode().unwrap());
    }

    #[test]
    fn guessing_passcodes_locks_the_pc_out() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        for _ in 0..MAX_FAILED_AUTH {
            let (cid, _rx) = connect_from(&mut hub, "192.168.1.20");
            assert!(!hub.handle(cid, assistant_hello("AAA-AAA")));
        }
        // Even the right passcode is refused from that PC now...
        let (cid, mut rx) = connect_from(&mut hub, "192.168.1.20");
        assert!(!hub.handle(cid, assistant_hello("K7Q-4MZ")));
        assert!(hub.assistant_name(cid).is_none());
        assert!(inbox(&mut rx)[0]["message"].as_str().unwrap().contains("Too many"));
        // ...but the real assistant on another PC can still log in.
        let (back, _rx) = connect_from(&mut hub, "192.168.1.40");
        assert!(hub.handle(back, assistant_hello("K7Q-4MZ")));
    }

    #[test]
    fn wrong_passcode_is_rejected_and_closes() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (cid, mut rx) = connect(&mut hub);
        assert!(!hub.handle(cid, assistant_hello("AAA-AAA")));
        assert!(hub.assistant_name(cid).is_none());
        assert_eq!(inbox(&mut rx)[0]["type"], "auth_error");
    }

    #[test]
    fn right_passcode_works_even_lowercase() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (cid, mut rx) = connect(&mut hub);
        assert!(hub.handle(cid, assistant_hello(" k7q-4mz ")));
        assert_eq!(hub.assistant_name(cid).as_deref(), Some("Front"));
        assert_eq!(inbox(&mut rx)[0]["type"], "auth_ok");
    }

    #[test]
    fn only_the_app_window_and_non_browsers_may_connect() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://tauri.localhost")));
        assert!(origin_allowed(Some("tauri://localhost")));
        assert!(!origin_allowed(Some("https://evil.example")));
        assert!(!origin_allowed(Some("http://tauri.localhost.evil.example")));
        assert!(!origin_allowed(Some("null")));
    }

    #[test]
    fn a_window_that_stops_reading_is_disconnected() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (slow, _never_read) = connect(&mut hub);
        let (ok, mut ok_rx) = connect(&mut hub);
        hub.handle(slow, ClientMsg::HelloStudent { pc: 3 });
        hub.handle(ok, ClientMsg::HelloStudent { pc: 4 });
        for _ in 0..=OUTBOX_SIZE {
            hub.broadcast();
            inbox(&mut ok_rx);
        }
        assert!(!hub.clients.contains_key(&slow));
        assert!(hub.clients.contains_key(&ok));
    }

    #[test]
    fn connection_limits_per_computer_and_total() {
        let limits = Arc::new(ConnLimits::default());
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        let slots: Vec<_> = (0..MAX_CONNECTIONS_PER_IP).map(|_| limits.try_take(ip).unwrap()).collect();
        assert!(limits.try_take(ip).is_none());
        drop(slots);
        assert!(limits.try_take(ip).is_some());
        // The hub PC can open many (demo script), up to the total limit.
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let many: Vec<_> = (0..MAX_CONNECTIONS).map_while(|_| limits.try_take(local)).collect();
        assert_eq!(many.len(), MAX_CONNECTIONS);
        assert!(limits.try_take(ip).is_none());
    }

    #[test]
    fn passcode_without_dash_works() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (cid, _rx) = connect(&mut hub);
        assert!(hub.handle(cid, assistant_hello("k7q4mz")));
        assert_eq!(hub.assistant_name(cid).as_deref(), Some("Front"));
    }

    #[test]
    fn unknown_seat_name_is_rejected() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (cid, _rx) = connect(&mut hub);
        let hello = ClientMsg::HelloAssistant { name: "x".repeat(10_000), passcode: "K7Q-4MZ".into() };
        assert!(!hub.handle(cid, hello));
        assert!(hub.assistant_name(cid).is_none());
    }

    fn student_hello(hub: &mut Hub, ip: &str, pc: u32) -> (u64, bool) {
        let (cid, _rx) = connect_from(hub, ip);
        let ok = hub.handle(cid, ClientMsg::HelloStudent { pc });
        (cid, ok)
    }

    #[test]
    fn admin_and_out_of_range_pc_numbers_are_rejected() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        for pc in [0, 1, 40, 41, u32::MAX] {
            let (cid, ok) = student_hello(&mut hub, "192.168.1.20", pc);
            assert!(!ok, "PC {pc} should be rejected");
            assert!(hub.student_pc(cid).is_none());
        }
    }

    #[test]
    fn another_computer_cannot_pose_as_an_online_pc() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (real, ok) = student_hello(&mut hub, "192.168.1.7", 7);
        assert!(ok);
        hub.handle(real, ClientMsg::Flare { question: "secret".into() });

        let (fake, ok) = student_hello(&mut hub, "192.168.1.9", 7);
        assert!(!ok);
        hub.handle(fake, ClientMsg::Cancel);
        assert_eq!(hub.flares.len(), 1);

        // Still protected after the real PC disconnects, while its Flare is open.
        hub.clients.remove(&real);
        assert!(!student_hello(&mut hub, "192.168.1.9", 7).1);
        // The real PC can reconnect and keeps its place.
        assert!(student_hello(&mut hub, "192.168.1.7", 7).1);
    }

    #[test]
    fn one_computer_cannot_be_two_pcs_at_once() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (first, ok) = student_hello(&mut hub, "192.168.1.9", 9);
        assert!(ok);
        assert!(!student_hello(&mut hub, "192.168.1.9", 12).1);
        // After Exit (disconnect, no Flare) it may pick another number.
        hub.clients.remove(&first);
        assert!(student_hello(&mut hub, "192.168.1.9", 12).1);
    }

    #[test]
    fn hub_pc_itself_can_simulate_many_students() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        assert!(student_hello(&mut hub, "127.0.0.1", 5).1);
        assert!(student_hello(&mut hub, "127.0.0.1", 6).1);
    }

    #[test]
    fn student_cannot_become_assistant_later() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (cid, _rx) = connect(&mut hub);
        hub.handle(cid, ClientMsg::HelloStudent { pc: 7 });
        hub.handle(cid, assistant_hello("K7Q-4MZ"));
        assert!(hub.assistant_name(cid).is_none());
        assert_eq!(hub.student_pc(cid), Some(7));
    }

    #[test]
    fn students_cannot_claim_or_resolve() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (s, _rx) = connect(&mut hub);
        hub.handle(s, ClientMsg::HelloStudent { pc: 7 });
        hub.handle(s, ClientMsg::Flare { question: "help".into() });
        let id = hub.flares[0].id;
        hub.handle(s, ClientMsg::Claim { id });
        hub.handle(s, ClientMsg::Resolve { id });
        assert_eq!(hub.flares.len(), 1);
        assert!(hub.flares[0].claimed_by.is_none());
    }

    #[test]
    fn only_assistants_see_the_queue_and_pc_numbers() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (a, mut a_rx) = connect(&mut hub);
        let (s7, mut s7_rx) = connect(&mut hub);
        let (s9, _s9_rx) = connect(&mut hub);
        let (stranger, mut stranger_rx) = connect(&mut hub); // never said hello
        hub.handle(a, assistant_hello("K7Q-4MZ"));
        hub.handle(s7, ClientMsg::HelloStudent { pc: 7 });
        hub.handle(s9, ClientMsg::HelloStudent { pc: 9 });
        hub.handle(s9, ClientMsg::Flare { question: "first".into() });
        hub.handle(s7, ClientMsg::Flare { question: "second".into() });
        hub.broadcast();

        // Assistant sees both Flares with PC numbers, oldest first.
        let a_state = inbox(&mut a_rx).into_iter().find(|m| m["type"] == "state").unwrap();
        assert_eq!(a_state["flares"][0]["pc"], 9);
        assert_eq!(a_state["flares"][1]["pc"], 7);

        // Student 7 only sees its own Flare and place in line, no PC numbers.
        let s7_view = inbox(&mut s7_rx).pop().unwrap();
        assert_eq!(s7_view["type"], "my_flare");
        assert_eq!(s7_view["flare"]["position"], 2);
        assert!(!s7_view.to_string().contains("\"pc\""));

        // A window that never identified itself gets nothing.
        assert!(inbox(&mut stranger_rx).is_empty());
        let _ = stranger;
    }

    #[test]
    fn two_assistants_cannot_claim_the_same_flare() {
        let mut hub = Hub::new("K7Q-4MZ".into());
        let (front, _r1) = connect(&mut hub);
        let (back, _r2) = connect(&mut hub);
        let (s, _r3) = connect(&mut hub);
        hub.handle(front, assistant_hello("K7Q-4MZ"));
        hub.handle(back, ClientMsg::HelloAssistant { name: "Back".into(), passcode: "K7Q-4MZ".into() });
        hub.handle(s, ClientMsg::HelloStudent { pc: 12 });
        hub.handle(s, ClientMsg::Flare { question: "".into() });
        let id = hub.flares[0].id;
        hub.handle(front, ClientMsg::Claim { id });
        hub.handle(back, ClientMsg::Claim { id });
        assert_eq!(hub.flares[0].claimed_by.as_deref(), Some("Front"));
    }
}

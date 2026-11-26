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
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

/// TCP port the hub listens on. Every PC must use the same port.
pub const PORT: u16 = 47800;

/// Longest question a student can type (characters).
const MAX_QUESTION_LEN: usize = 200;

/// One help request from one student PC.
#[derive(Clone, Serialize)]
struct Flare {
    id: u64,
    pc: u32,
    question: String,
    created_ms: u64,
    /// Name of the assistant who clicked "I'll take it", e.g. "Front".
    claimed_by: Option<String>,
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
    /// Messages pushed here are written to this client's WebSocket.
    tx: mpsc::UnboundedSender<Message>,
}

struct Hub {
    /// Secret an assistant must send to be treated as an assistant.
    /// Without it a tampered student app sees nothing and controls nothing.
    passcode: String,
    next_client_id: u64,
    next_flare_id: u64,
    clients: HashMap<u64, Client>,
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

/// Make a short random passcode like "K7Q-4MZ".
/// Uses letters and digits that are hard to confuse (no 0/O, 1/I/L).
/// Randomness comes from Rust's hasher seed, which the OS randomizes per run.
fn new_passcode() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut out = String::new();
    for i in 0..6u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i ^ now_ms());
        let idx = (h.finish() % ALPHABET.len() as u64) as usize;
        if i == 3 {
            out.push('-');
        }
        out.push(ALPHABET[idx] as char);
    }
    out
}

impl Hub {
    fn new(passcode: String) -> Self {
        Hub { passcode, next_client_id: 0, next_flare_id: 0, clients: HashMap::new(), flares: Vec::new() }
    }

    /// Apply one message from client `cid` to the hub state.
    /// Returns false if the connection should be closed (wrong passcode).
    fn handle(&mut self, cid: u64, msg: ClientMsg) -> bool {
        match msg {
            ClientMsg::HelloStudent { pc } => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    // A window picks its role once; it cannot switch later.
                    if matches!(c.role, Role::Unknown) {
                        c.role = Role::Student(pc);
                    }
                }
            }
            ClientMsg::HelloAssistant { name, passcode } => {
                let Some(c) = self.clients.get_mut(&cid) else { return false };
                if !matches!(c.role, Role::Unknown) {
                    return true;
                }
                if passcode.trim().to_uppercase() != self.passcode {
                    let _ = c.tx.send(Message::text(
                        json!({ "type": "auth_error", "message": "Wrong passcode" }).to_string(),
                    ));
                    return false;
                }
                c.role = Role::Assistant(name);
                let _ = c.tx.send(Message::text(json!({ "type": "auth_ok" }).to_string()));
            }
            ClientMsg::Flare { question } => {
                let Some(pc) = self.student_pc(cid) else { return true };
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
                let _ = c.tx.send(Message::text(text.clone()));
            }
        }
    }

    /// Send every client its own view of the current state.
    fn broadcast(&self) {
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

        for c in self.clients.values() {
            match &c.role {
                Role::Assistant(_) => {
                    let _ = c.tx.send(Message::text(assistant_view.clone()));
                }
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
                    let _ = c.tx.send(Message::text(view.to_string()));
                }
                Role::Unknown => {}
            }
        }
    }
}

/// Start listening on every network interface. Returns the session passcode
/// once the port is open, then keeps accepting connections in the background.
pub async fn start() -> Result<String, String> {
    let listener = TcpListener::bind(("0.0.0.0", PORT))
        .await
        .map_err(|e| format!("could not open port {PORT}: {e}"))?;

    let passcode = new_passcode();
    let hub: SharedHub = Arc::new(Mutex::new(Hub::new(passcode.clone())));
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok((stream, _addr)) = listener.accept().await {
                tauri::async_runtime::spawn(handle_connection(stream, hub.clone()));
            }
        }
    });
    Ok(passcode)
}

/// Serve one connected window until it disconnects.
async fn handle_connection(stream: TcpStream, hub: SharedHub) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
    let (mut ws_out, mut ws_in) = ws.split();

    // Register the client with a channel we can push messages into.
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let cid = {
        let mut h = hub.lock().await;
        h.next_client_id += 1;
        let cid = h.next_client_id;
        h.clients.insert(cid, Client { role: Role::Unknown, tx });
        cid
    };

    // Writer task: forward queued messages to the socket.
    let writer = tauri::async_runtime::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_out.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Reader loop: apply each incoming message, then update everyone.
    while let Some(Ok(msg)) = ws_in.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(parsed) = serde_json::from_str::<ClientMsg>(&text) else { continue };
        let mut h = hub.lock().await;
        let keep_open = h.handle(cid, parsed);
        h.broadcast();
        drop(h);
        if !keep_open {
            // Wrong passcode: wait a moment before closing, so guessing
            // passcodes one by one would take far too long.
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            break;
        }
    }

    // Disconnected. The student's Flare (if any) stays in the queue on purpose,
    // so a crash or network blip does not lose their place.
    let mut h = hub.lock().await;
    h.clients.remove(&cid); // drops this client's sender, so the writer finishes
    h.broadcast();
    drop(h);
    let _ = writer.await; // let queued messages (e.g. "Wrong passcode") go out first
}

// ===== Tests =====
// Run with: cargo test   (inside app/src-tauri)
// These check the hub's rules directly, without a real network.
#[cfg(test)]
mod tests {
    use super::*;

    /// Add a fake connected window and return its id plus its inbox.
    fn connect(hub: &mut Hub) -> (u64, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        hub.next_client_id += 1;
        let cid = hub.next_client_id;
        hub.clients.insert(cid, Client { role: Role::Unknown, tx });
        (cid, rx)
    }

    /// Everything a window has received so far, as JSON values.
    fn inbox(rx: &mut mpsc::UnboundedReceiver<Message>) -> Vec<serde_json::Value> {
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
        let p = new_passcode();
        assert_eq!(p.len(), 7);
        assert_eq!(&p[3..4], "-");
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

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
    HelloAssistant { name: String },
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

#[derive(Default)]
struct Hub {
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

impl Hub {
    /// Apply one message from client `cid` to the hub state.
    fn handle(&mut self, cid: u64, msg: ClientMsg) {
        match msg {
            ClientMsg::HelloStudent { pc } => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.role = Role::Student(pc);
                }
            }
            ClientMsg::HelloAssistant { name } => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.role = Role::Assistant(name);
                }
            }
            ClientMsg::Flare { question } => {
                let Some(pc) = self.student_pc(cid) else { return };
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
                let Some(name) = self.assistant_name(cid) else { return };
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
                    return;
                }
                if let Some(f) = self.flares.iter_mut().find(|f| f.id == id) {
                    f.claimed_by = None;
                }
            }
            ClientMsg::Resolve { id } => {
                if self.assistant_name(cid).is_none() {
                    return;
                }
                if let Some(pos) = self.flares.iter().position(|f| f.id == id) {
                    let pc = self.flares.remove(pos).pc;
                    // Tell that student their question was answered.
                    self.send_to_pc(pc, json!({ "type": "resolved" }));
                }
            }
        }
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

/// Start listening on every network interface. Returns once the port is open,
/// then keeps accepting connections in the background.
pub async fn start() -> Result<(), String> {
    let listener = TcpListener::bind(("0.0.0.0", PORT))
        .await
        .map_err(|e| format!("could not open port {PORT}: {e}"))?;

    let hub: SharedHub = Arc::new(Mutex::new(Hub::default()));
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok((stream, _addr)) = listener.accept().await {
                tauri::async_runtime::spawn(handle_connection(stream, hub.clone()));
            }
        }
    });
    Ok(())
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
        h.handle(cid, parsed);
        h.broadcast();
    }

    // Disconnected. The student's Flare (if any) stays in the queue on purpose,
    // so a crash or network blip does not lose their place.
    let mut h = hub.lock().await;
    h.clients.remove(&cid);
    h.broadcast();
    drop(h);
    writer.abort();
}

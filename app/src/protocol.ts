// The "dictionary" of messages between the screens and the hub (hub.rs).
// Field names must match the Rust side exactly.

/** Port the hub listens on (hub::PORT in Rust). */
export const HUB_PORT = 47800;

/** One open help request, as assistants see it. */
export type Flare = {
  id: number;
  pc: number;
  question: string;
  created_ms: number;
  claimed_by: string | null;
};

/** A student's view of their own Flare. Never contains other students' info. */
export type MyFlare = {
  question: string;
  position: number;
  claimed_by: string | null;
};

/** Messages the hub sends to us. */
export type HubMessage =
  | { type: "state"; now_ms: number; flares: Flare[]; online: number[] } // assistants only
  | { type: "my_flare"; flare: MyFlare | null } // students only
  | { type: "resolved" } // students: your Flare was answered
  | { type: "auth_ok" }
  | { type: "auth_error"; message: string };

/** Messages we send to the hub. */
export type ClientMessage =
  | { type: "hello_student"; pc: number }
  | { type: "hello_assistant"; name: string; passcode: string }
  | { type: "flare"; question: string }
  | { type: "cancel" }
  | { type: "claim"; id: number }
  | { type: "release"; id: number }
  | { type: "resolve"; id: number };

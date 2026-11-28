import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Logo from "../components/Logo";
import { FIRST_STUDENT_PC, LabConfig, LAST_STUDENT_PC, Seat, SetupConfig } from "../types";

type Role = SetupConfig["role"];

type Errors = { pc?: string; hub?: string; passcode?: string };

/**
 * What the setup screen shows:
 * - search: looking for the hub on the network
 * - quick:  one hub found, the student only types their PC number
 * - pick:   more than one hub answered, the student must choose
 * - full:   the complete form (assistants, or typing the address by hand)
 */
type Mode = "search" | "quick" | "pick" | "full";

/** Wait between two searches while no hub answers. One search takes ~1.5 s. */
const SEARCH_AGAIN_MS = 1500;

// Accepts "localhost" or an IPv4 address like 192.168.1.10
const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;
function isValidAddress(text: string) {
  return text === "localhost" || IPV4.test(text);
}

// Passcodes look like K7Q-4MZ (the dash is optional when typing)
const PASSCODE = /^[A-Z0-9]{3}-?[A-Z0-9]{3}$/i;

type Props = {
  onStart: (config: SetupConfig) => void;
  /** Answers from the last Start, so pressing Exit does not wipe the form. */
  previous: SetupConfig | null;
};

export default function SetupScreen({ onStart, previous }: Props) {
  // Lab config: undefined = still loading, null = none found (developer laptop).
  const [lab, setLab] = useState<LabConfig | null | undefined>(undefined);
  const [labError, setLabError] = useState("");

  // Form state: one useState per field. The passcode is never kept.
  const [role, setRole] = useState<Role>(previous?.role ?? "student");
  const [pc, setPc] = useState(previous?.role === "student" ? String(previous.pc) : "2");
  const [seat, setSeat] = useState<Seat>(previous?.role === "assistant" ? previous.seat : "Front");
  const [hostHub, setHostHub] = useState(previous?.role === "assistant" ? previous.hostHub : true);
  const [hubAddress, setHubAddress] = useState(previous?.hubAddress ?? "127.0.0.1");
  const [passcode, setPasscode] = useState("");
  const [errors, setErrors] = useState<Errors>({});
  // undefined until the lab config has loaded.
  const [mode, setMode] = useState<Mode | undefined>(undefined);
  // Hubs that answered the last search (pick mode).
  const [hubs, setHubs] = useState<string[]>([]);

  // On startup, ask Rust for the lab config file. If it exists, it decides the role.
  useEffect(() => {
    invoke<LabConfig | null>("load_config")
      .then((config) => {
        setLab(config);
        // Search for the hub, unless the lab config names it (safer in a real
        // lab) or the user just pressed Exit (keep their answers instead).
        const search = !previous && (!config || (config.role === "student" && !config.hub_address));
        setMode(search ? "search" : "full");
        if (!config) return;
        setRole(config.role);
        if (config.seat) chooseSeat(config.seat);
        if (config.pc) setPc(String(config.pc));
        if (config.hub_address) setHubAddress(config.hub_address);
      })
      .catch((e) => setLabError(String(e)));
  }, []);

  // While searching, ask the network again and again until a hub answers.
  useEffect(() => {
    if (mode !== "search") return;
    let stopped = false;
    let timer: number | undefined;

    async function search() {
      const found = await invoke<string[]>("find_hubs").catch(() => []);
      if (stopped) return;
      if (found.length === 0) {
        timer = window.setTimeout(search, SEARCH_AGAIN_MS);
      } else if (found.length === 1) {
        joinHub(found[0]);
      } else {
        // Two hubs: one may be a fake run by a classmate. Let a person decide.
        setHubs(found);
        setMode("pick");
      }
    }

    search();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [mode]);

  /** A hub was found (or picked): join it as a student. */
  function joinHub(address: string) {
    setHubAddress(address);
    setRole("student");
    // The lab config already knows this PC's number: join with no click.
    if (lab?.pc) {
      onStart({ role: "student", pc: lab.pc, hubAddress: address });
      return;
    }
    // Start empty: a pre-filled number would let students join as the wrong PC.
    setPc("");
    setMode("quick");
  }

  function pcError(text: string) {
    const n = Number(text);
    const ok = Number.isInteger(n) && n >= FIRST_STUDENT_PC && n <= LAST_STUDENT_PC;
    return ok ? undefined : `Enter a student PC number from ${FIRST_STUDENT_PC} to ${LAST_STUDENT_PC}.`;
  }

  function handleJoin(e: React.FormEvent) {
    e.preventDefault();
    const problem = pcError(pc);
    setErrors({ pc: problem });
    if (!problem) onStart({ role: "student", pc: Number(pc), hubAddress });
  }

  // The front assistant hosts the hub, the back one connects to it.
  function chooseSeat(next: Seat) {
    setSeat(next);
    setHostHub(next === "Front");
  }

  const locked = !!lab; // role and seat come from the lab config
  const needsAddress = role === "student" || !hostHub;
  const addressFromConfig = !!lab?.hub_address;
  const pcFromConfig = !!lab?.pc;
  const needsPasscode = role === "assistant" && !hostHub;

  function handleStart(e: React.FormEvent) {
    e.preventDefault();
    const found: Errors = {};
    const address = hubAddress.trim();
    const pcNumber = Number(pc);

    if (role === "student") found.pc = pcError(pc);
    if (needsAddress && !isValidAddress(address)) {
      found.hub = "Enter an address like 192.168.1.10 (shown on the front assistant's screen).";
    }
    if (needsPasscode && !PASSCODE.test(passcode.trim())) {
      found.passcode = "Enter the 6-character passcode shown on the front assistant's screen, e.g. K7Q-4MZ.";
    }

    setErrors(found);
    if (Object.values(found).some(Boolean)) return;

    onStart(
      role === "student"
        ? { role, pc: pcNumber, hubAddress: address }
        : {
            role,
            seat,
            hostHub,
            hubAddress: hostHub ? "127.0.0.1" : address,
            passcode: needsPasscode ? passcode.trim().toUpperCase() : "",
          },
    );
  }

  // A broken config file on a lab PC must not fall back to the role picker,
  // or students would see the assistant option. Show the problem instead.
  if (labError) {
    return (
      <main className="setup">
        <div className="card setup-card" role="alert">
          <h1>Mercusoar is not set up correctly</h1>
          <p className="error">{labError}</p>
          <p className="help">Please tell the lab assistant.</p>
        </div>
      </main>
    );
  }

  if (lab === undefined || mode === undefined) return null; // loading, takes a moment

  const brand = (
    <div className="brand-row">
      <Logo size={40} />
      <div>
        <h1 className="brand-name">Mercusoar</h1>
        <p className="tagline">Light the way, not the spotlight.</p>
      </div>
    </div>
  );

  // Leave the automatic flow for the complete form.
  const manualLabel = locked ? "Type the hub address instead" : "Lab assistant, or type the address by hand";
  const manualButton = (
    <button type="button" className="btn-link" onClick={() => setMode("full")}>
      {manualLabel}
    </button>
  );

  if (mode === "search") {
    return (
      <main className="setup">
        <div className="card setup-card">
          {brand}
          <div className="search-state" role="status">
            <span className="search-dot" aria-hidden="true" />
            <div>
              <p className="search-title">Looking for the lab hub…</p>
              <p className="help">
                This joins automatically once the front assistant has started Mercusoar.
              </p>
            </div>
          </div>
          {manualButton}
        </div>
      </main>
    );
  }

  if (mode === "pick") {
    return (
      <main className="setup">
        <div className="card setup-card">
          {brand}
          <div className="hub-warning" role="alert">
            <p className="search-title">More than one hub answered</p>
            <p className="help">
              Only one is the real lab hub. Ask the lab assistant which address is shown on their screen.
            </p>
          </div>
          <div className="hub-list">
            {hubs.map((address) => (
              <button key={address} type="button" className="hub-choice" onClick={() => joinHub(address)}>
                {address}
              </button>
            ))}
          </div>
          <button type="button" className="btn-link" onClick={() => setMode("search")}>
            Search again
          </button>
          {manualButton}
        </div>
      </main>
    );
  }

  if (mode === "quick") {
    return (
      <main className="setup">
        <form className="card setup-card" onSubmit={handleJoin} noValidate>
          {brand}
          <p className="hub-found">
            <span aria-hidden="true">✓</span> Hub found <span className="muted">({hubAddress})</span>
          </p>
          <div className="field">
            <label htmlFor="quick-pc">Your PC number</label>
            <input
              id="quick-pc"
              type="number"
              inputMode="numeric"
              min={FIRST_STUDENT_PC}
              max={LAST_STUDENT_PC}
              value={pc}
              onChange={(e) => setPc(e.target.value)}
              aria-invalid={!!errors.pc}
              aria-describedby="quick-pc-help"
              autoFocus
            />
            {errors.pc ? (
              <p id="quick-pc-help" className="error">{errors.pc}</p>
            ) : (
              <p id="quick-pc-help" className="help">
                The number on the sticker of this PC ({FIRST_STUDENT_PC} to {LAST_STUDENT_PC}).
              </p>
            )}
          </div>
          <button type="submit" className="btn-primary">Join</button>
          <button type="button" className="btn-link" onClick={() => setMode("full")}>
            {locked ? "Type the hub address instead" : "Not a student? Set up manually"}
          </button>
        </form>
      </main>
    );
  }

  return (
    <main className="setup">
      <form className="card setup-card" onSubmit={handleStart} noValidate>
        {brand}

        {!locked && (
          <p className="dev-note">
            Developer mode: no lab config found, so any role can be picked. On lab PCs the role is fixed.
          </p>
        )}

        {!locked && (
          <fieldset className="field">
            <legend>Who is using this PC?</legend>
            <div className="role-options">
              <label className={`role-option ${role === "student" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="role"
                  value="student"
                  checked={role === "student"}
                  onChange={() => setRole("student")}
                />
                <span className="role-title">Student</span>
                <span className="role-desc">Ask for help quietly</span>
              </label>
              <label className={`role-option ${role === "assistant" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="role"
                  value="assistant"
                  checked={role === "assistant"}
                  onChange={() => setRole("assistant")}
                />
                <span className="role-title">Lab assistant</span>
                <span className="role-desc">See and answer Flares</span>
              </label>
            </div>
          </fieldset>
        )}

        {role === "student" && pcFromConfig && <p className="fixed-seat">Student Â· PC {pc}</p>}

        {role === "student" && !pcFromConfig && (
          <div className="field">
            <label htmlFor="pc">Your PC number</label>
            <input
              id="pc"
              type="number"
              inputMode="numeric"
              min={FIRST_STUDENT_PC}
              max={LAST_STUDENT_PC}
              value={pc}
              onChange={(e) => setPc(e.target.value)}
              aria-invalid={!!errors.pc}
              aria-describedby="pc-help"
            />
            {errors.pc ? (
              <p id="pc-help" className="error">{errors.pc}</p>
            ) : (
              <p id="pc-help" className="help">
                Student PCs are {FIRST_STUDENT_PC} to {LAST_STUDENT_PC}. PC 1 and PC 40 are admin PCs.
              </p>
            )}
          </div>
        )}

        {role === "assistant" && locked && (
          <p className="fixed-seat">
            Lab assistant Â· {seat} seat (PC {seat === "Front" ? 1 : 40})
            {hostHub && " Â· hosts the hub"}
          </p>
        )}

        {role === "assistant" && !locked && (
          <>
            <fieldset className="field">
              <legend>Your seat</legend>
              <div className="seat-options">
                {(["Front", "Back"] as Seat[]).map((s) => (
                  <label key={s} className={`seat-option ${seat === s ? "selected" : ""}`}>
                    <input
                      type="radio"
                      name="seat"
                      value={s}
                      checked={seat === s}
                      onChange={() => chooseSeat(s)}
                    />
                    {s} <span className="muted">(PC {s === "Front" ? 1 : 40})</span>
                  </label>
                ))}
              </div>
            </fieldset>

            <div className="field check">
              <input
                id="host-hub"
                type="checkbox"
                checked={hostHub}
                onChange={(e) => setHostHub(e.target.checked)}
              />
              <label htmlFor="host-hub">Host the hub on this PC</label>
            </div>
            <p className="help hub-note">
              Only one PC hosts the hub, usually the front admin PC. Everyone else connects to it.
            </p>
          </>
        )}

        {needsAddress && !addressFromConfig && (
          <div className="field">
            <label htmlFor="hub">Hub address</label>
            <input
              id="hub"
              type="text"
              value={hubAddress}
              onChange={(e) => setHubAddress(e.target.value)}
              aria-invalid={!!errors.hub}
              aria-describedby="hub-help"
              spellCheck={false}
            />
            {errors.hub ? (
              <p id="hub-help" className="error">{errors.hub}</p>
            ) : (
              <p id="hub-help" className="help">
                Shown on the front assistant's screen. Use 127.0.0.1 when testing on one PC.
              </p>
            )}
          </div>
        )}

        {needsPasscode && (
          <div className="field">
            <label htmlFor="passcode">Assistant passcode</label>
            <input
              id="passcode"
              type="text"
              value={passcode}
              onChange={(e) => setPasscode(e.target.value)}
              aria-invalid={!!errors.passcode}
              aria-describedby="passcode-help"
              autoComplete="off"
              spellCheck={false}
              placeholder="K7Q-4MZ"
            />
            {errors.passcode ? (
              <p id="passcode-help" className="error">{errors.passcode}</p>
            ) : (
              <p id="passcode-help" className="help">
                Shown only on the front assistant's screen. It changes every session.
              </p>
            )}
          </div>
        )}

        <button type="submit" className="btn-primary">Start</button>
      </form>
    </main>
  );
}

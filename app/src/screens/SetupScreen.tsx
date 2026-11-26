import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Logo from "../components/Logo";
import { FIRST_STUDENT_PC, LabConfig, LAST_STUDENT_PC, Seat, SetupConfig } from "../types";

type Role = SetupConfig["role"];

type Errors = { pc?: string; hub?: string; passcode?: string };

// Accepts "localhost" or an IPv4 address like 192.168.1.10
const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;
function isValidAddress(text: string) {
  return text === "localhost" || IPV4.test(text);
}

// Passcodes look like K7Q-4MZ (the dash is optional when typing)
const PASSCODE = /^[A-Z0-9]{3}-?[A-Z0-9]{3}$/i;

export default function SetupScreen({ onStart }: { onStart: (config: SetupConfig) => void }) {
  // Lab config: undefined = still loading, null = none found (developer laptop).
  const [lab, setLab] = useState<LabConfig | null | undefined>(undefined);
  const [labError, setLabError] = useState("");

  // Form state: one useState per field.
  const [role, setRole] = useState<Role>("student");
  const [pc, setPc] = useState("2");
  const [seat, setSeat] = useState<Seat>("Front");
  const [hostHub, setHostHub] = useState(true);
  const [hubAddress, setHubAddress] = useState("127.0.0.1");
  const [passcode, setPasscode] = useState("");
  const [errors, setErrors] = useState<Errors>({});

  // On startup, ask Rust for the lab config file. If it exists, it decides the role.
  useEffect(() => {
    invoke<LabConfig | null>("load_config")
      .then((config) => {
        setLab(config);
        if (!config) return;
        setRole(config.role);
        if (config.seat) chooseSeat(config.seat);
        if (config.hub_address) setHubAddress(config.hub_address);
      })
      .catch((e) => setLabError(String(e)));
  }, []);

  // The front assistant hosts the hub, the back one connects to it.
  function chooseSeat(next: Seat) {
    setSeat(next);
    setHostHub(next === "Front");
  }

  const locked = !!lab; // role and seat come from the lab config
  const needsAddress = role === "student" || !hostHub;
  const addressFromConfig = !!lab?.hub_address;
  const needsPasscode = role === "assistant" && !hostHub;

  function handleStart(e: React.FormEvent) {
    e.preventDefault();
    const found: Errors = {};
    const address = hubAddress.trim();
    const pcNumber = Number(pc);

    if (role === "student") {
      const ok = Number.isInteger(pcNumber) && pcNumber >= FIRST_STUDENT_PC && pcNumber <= LAST_STUDENT_PC;
      if (!ok) found.pc = `Enter a student PC number from ${FIRST_STUDENT_PC} to ${LAST_STUDENT_PC}.`;
    }
    if (needsAddress && !isValidAddress(address)) {
      found.hub = "Enter an address like 192.168.1.10 (shown on the front assistant's screen).";
    }
    if (needsPasscode && !PASSCODE.test(passcode.trim())) {
      found.passcode = "Enter the 6-character passcode shown on the front assistant's screen, e.g. K7Q-4MZ.";
    }

    setErrors(found);
    if (Object.keys(found).length > 0) return;

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

  if (lab === undefined) return null; // loading, takes a moment

  return (
    <main className="setup">
      <form className="card setup-card" onSubmit={handleStart} noValidate>
        <div className="brand-row">
          <Logo size={40} />
          <div>
            <h1 className="brand-name">Mercusoar</h1>
            <p className="tagline">Light the way, not the spotlight.</p>
          </div>
        </div>

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

        {role === "student" && (
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
            Lab assistant · {seat} seat (PC {seat === "Front" ? 1 : 40})
            {hostHub && " · hosts the hub"}
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

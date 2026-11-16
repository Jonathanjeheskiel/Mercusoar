import { useState } from "react";
import Logo from "../components/Logo";
import { FIRST_STUDENT_PC, LAST_STUDENT_PC, Seat, SetupConfig } from "../types";

type Role = SetupConfig["role"];

type Errors = { pc?: string; hub?: string };

// Accepts "localhost" or an IPv4 address like 192.168.1.10
const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;
function isValidAddress(text: string) {
  return text === "localhost" || IPV4.test(text);
}

export default function SetupScreen({ onStart }: { onStart: (config: SetupConfig) => void }) {
  // Form state: one useState per field.
  const [role, setRole] = useState<Role>("student");
  const [pc, setPc] = useState("2");
  const [seat, setSeat] = useState<Seat>("Front");
  const [hostHub, setHostHub] = useState(true);
  const [hubAddress, setHubAddress] = useState("127.0.0.1");
  const [errors, setErrors] = useState<Errors>({});

  // The front assistant normally hosts the hub, the back one connects to it.
  function chooseSeat(next: Seat) {
    setSeat(next);
    setHostHub(next === "Front");
  }

  // Students always need the hub address. Assistants only if they don't host it.
  const needsAddress = role === "student" || !hostHub;

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
      found.hub = "Enter an address like 192.168.1.10 (ask the assistant at PC 1).";
    }

    setErrors(found);
    if (Object.keys(found).length > 0) return;

    onStart(
      role === "student"
        ? { role, pc: pcNumber, hubAddress: address }
        : { role, seat, hostHub, hubAddress: hostHub ? "127.0.0.1" : address },
    );
  }

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

        {role === "student" && (
          <div className="field">
            <label htmlFor="pc">PC number</label>
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

        {role === "assistant" && (
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

        {needsAddress && (
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

        <button type="submit" className="btn-primary">Start</button>
      </form>
    </main>
  );
}

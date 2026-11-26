import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ConnectionBadge from "../components/ConnectionBadge";
import LabChart from "../components/LabChart";
import Logo from "../components/Logo";
import ProjectorView from "../components/ProjectorView";
import { Flare, HubMessage } from "../protocol";
import { Seat } from "../types";
import { useHub } from "../useHub";

type Props = {
  seat: Seat;
  hostHub: boolean;
  hubAddress: string;
  passcode: string;
  onExit: () => void;
};

type HubInfo = { address: string; passcode: string };

export default function AssistantScreen({ seat, hostHub, hubAddress, passcode, onExit }: Props) {
  // When this PC hosts the hub, start it first and use its passcode.
  const [hubInfo, setHubInfo] = useState<HubInfo | null>(null);
  const [hubError, setHubError] = useState("");
  const [showPasscode, setShowPasscode] = useState(false);

  useEffect(() => {
    if (!hostHub) return;
    invoke<HubInfo>("start_hub")
      .then(setHubInfo)
      .catch((e) => setHubError(String(e)));
  }, [hostHub]);

  const address = hostHub ? (hubInfo ? "127.0.0.1" : null) : hubAddress;
  const code = hostHub ? (hubInfo?.passcode ?? "") : passcode;

  // Hub data
  const [flares, setFlares] = useState<Flare[]>([]);
  const [online, setOnline] = useState<number[]>([]);
  const clockOffset = useRef(0); // hub clock minus our clock, for fair wait times

  function onMessage(msg: HubMessage) {
    if (msg.type === "state") {
      setFlares(msg.flares);
      setOnline(msg.online);
      clockOffset.current = msg.now_ms - Date.now();
    }
  }

  const { status, send } = useHub(
    address,
    code ? { type: "hello_assistant", name: seat, passcode: code } : null,
    onMessage,
  );

  // Re-render every second so wait times count up.
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, []);
  const hubNow = Date.now() + clockOffset.current;

  // `at` changes on every click, so clicking the same seat again scrolls again.
  const [selected, setSelected] = useState<{ id: number; at: number } | null>(null);
  const [projected, setProjected] = useState<string | null>(null);

  const waiting = flares.filter((f) => f.claimed_by === null).length;

  return (
    <main className="screen">
      <header className="topbar">
        <span className="brand-small">
          <Logo size={24} /> Mercusoar
        </span>
        <span className="topbar-right">
          <span className="pc-pill">{seat} assistant</span>
          <ConnectionBadge status={status} />
          <button className="btn-link" onClick={onExit}>Exit</button>
        </span>
      </header>

      {hubError && (
        <p className="banner banner-error" role="alert">Could not start the hub: {hubError}</p>
      )}
      {status === "rejected" && (
        <p className="banner banner-error" role="alert">
          The hub rejected the passcode. Exit and type the passcode shown on the front assistant's screen.
        </p>
      )}

      {hubInfo && (
        <div className="hub-strip">
          <span>
            Hub address <strong className="mono">{hubInfo.address.split(":")[0]}</strong>
          </span>
          <span>
            Assistant passcode{" "}
            <strong className="mono">{showPasscode ? hubInfo.passcode : "•••-•••"}</strong>
            <button className="btn-link" onClick={() => setShowPasscode((s) => !s)}>
              {showPasscode ? "Hide" : "Show"}
            </button>
          </span>
          <span className="muted">Only give the passcode to the other lab assistant.</span>
        </div>
      )}

      <div className="assistant-layout">
        <section className="card" aria-labelledby="chart-title">
          <h2 id="chart-title">Lab Chart</h2>
          <LabChart flares={flares} online={online} onSelect={(id) => setSelected({ id, at: Date.now() })} />
        </section>

        <section className="card queue-card" aria-labelledby="queue-title">
          <h2 id="queue-title">
            Queue <span className="muted">{flares.length} open · {waiting} waiting</span>
          </h2>
          {flares.length === 0 ? (
            <p className="empty">No Flares right now. Calm seas.</p>
          ) : (
            <ol className="queue">
              {flares.map((f, i) => (
                <QueueItem
                  key={f.id}
                  flare={f}
                  position={i + 1}
                  waitMs={hubNow - f.created_ms}
                  mySeat={seat}
                  selectKey={selected?.id === f.id ? selected.at : null}
                  onClaim={() => send({ type: "claim", id: f.id })}
                  onRelease={() => send({ type: "release", id: f.id })}
                  onResolve={() => send({ type: "resolve", id: f.id })}
                  onProject={() => setProjected(f.question)}
                />
              ))}
            </ol>
          )}
        </section>
      </div>

      {projected !== null && (
        <ProjectorView question={projected} onClose={() => setProjected(null)} />
      )}
    </main>
  );
}

function formatWait(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

type ItemProps = {
  flare: Flare;
  position: number;
  waitMs: number;
  mySeat: Seat;
  /** Set when this item was picked on the Lab Chart; changes on every pick. */
  selectKey: number | null;
  onClaim: () => void;
  onRelease: () => void;
  onResolve: () => void;
  onProject: () => void;
};

function QueueItem(p: ItemProps) {
  const { flare } = p;
  const mine = flare.claimed_by === p.mySeat;
  const other = flare.claimed_by !== null && !mine;
  const ref = useRef<HTMLLIElement>(null);

  // Clicking a seat on the Lab Chart scrolls to its queue entry.
  useEffect(() => {
    if (p.selectKey !== null) ref.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [p.selectKey]);

  return (
    <li ref={ref} className={`queue-item ${p.selectKey !== null ? "selected" : ""} ${flare.claimed_by ? "claimed" : ""}`}>
      <div className="queue-head">
        <span className="queue-pos">#{p.position}</span>
        <span className="queue-pc">PC {flare.pc}</span>
        <span className="queue-wait" title="Waiting time">{formatWait(p.waitMs)}</span>
        {flare.claimed_by && <span className="queue-claim">{mine ? "You" : flare.claimed_by}</span>}
      </div>
      <p className={flare.question ? "queue-question" : "queue-question muted"}>
        {flare.question || "No question typed"}
      </p>
      <div className="queue-actions">
        {flare.claimed_by === null && (
          <button className="btn-small btn-amber" onClick={p.onClaim}>I'll take it</button>
        )}
        {mine && (
          <>
            <button className="btn-small btn-teal" onClick={p.onResolve}>Resolved</button>
            <button className="btn-small" onClick={p.onRelease}>Release</button>
          </>
        )}
        {other && <span className="muted small">Taken by the {flare.claimed_by} assistant</span>}
        {flare.question && (
          <button className="btn-small" onClick={p.onProject}>Explain at the front</button>
        )}
      </div>
    </li>
  );
}

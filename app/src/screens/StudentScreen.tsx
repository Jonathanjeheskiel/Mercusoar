import { useState } from "react";
import ConnectionBadge from "../components/ConnectionBadge";
import Logo from "../components/Logo";
import { HubMessage, MyFlare } from "../protocol";
import { useHub } from "../useHub";

const MAX_QUESTION = 200; // same limit as the hub

type Props = { pc: number; hubAddress: string; onExit: () => void };

export default function StudentScreen({ pc, hubAddress, onExit }: Props) {
  const [myFlare, setMyFlare] = useState<MyFlare | null>(null);
  const [justResolved, setJustResolved] = useState(false);
  const [question, setQuestion] = useState("");

  function onMessage(msg: HubMessage) {
    if (msg.type === "my_flare") setMyFlare(msg.flare);
    if (msg.type === "resolved") setJustResolved(true);
  }

  const { status, send, rejectReason } = useHub(hubAddress, { type: "hello_student", pc }, onMessage);
  const connected = status === "connected";

  function sendFlare(e: React.FormEvent) {
    e.preventDefault();
    send({ type: "flare", question: question.trim() });
    setJustResolved(false);
    setQuestion("");
  }

  return (
    <main className="screen">
      <header className="topbar">
        <span className="brand-small">
          <Logo size={24} /> Mercusoar
        </span>
        <span className="topbar-right">
          <span className="pc-pill">PC {pc}</span>
          <ConnectionBadge status={status} />
          <button className="btn-link" onClick={onExit}>Exit</button>
        </span>
      </header>

      {status === "rejected" && (
        <p className="banner banner-error" role="alert">
          {rejectReason || "The hub refused this PC."} Press Exit to start again, or tell the lab assistant.
        </p>
      )}

      <section className="student-center">
        <div className="card student-card" aria-live="polite">
          {myFlare ? (
            <FlareStatus flare={myFlare} onCancel={() => send({ type: "cancel" })} />
          ) : justResolved ? (
            <div className="status-block">
              <p className="status-eyebrow ok">Resolved</p>
              <h1>Glad you're unstuck!</h1>
              <p className="help">An assistant marked your Flare as answered.</p>
              <button className="btn-secondary" onClick={() => setJustResolved(false)}>
                Ask another question
              </button>
            </div>
          ) : (
            <form className="flare-form" onSubmit={sendFlare}>
              <h1>Stuck? Send a Flare.</h1>
              <p className="help">
                A lab assistant will help you. Your classmates won't know who asked.
              </p>
              <label htmlFor="question" className="field-label">
                What are you stuck on? <span className="muted">(optional)</span>
              </label>
              <textarea
                id="question"
                rows={3}
                maxLength={MAX_QUESTION}
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                placeholder="e.g. Task 3: my CSS file is not loading"
              />
              <p className="char-count">{question.length}/{MAX_QUESTION}</p>
              <button type="submit" className="btn-flare" disabled={!connected}>
                Send a Flare
              </button>
              {!connected && status !== "rejected" && <p className="help">Waiting for the hub connection…</p>}
            </form>
          )}
        </div>
        <p className="privacy-note">
          Only the lab assistants can see your PC number. Classmates never see who asked.
        </p>
      </section>
    </main>
  );
}

function FlareStatus({ flare, onCancel }: { flare: MyFlare; onCancel: () => void }) {
  const helping = flare.claimed_by !== null;
  return (
    <div className="status-block">
      <p className={`status-eyebrow ${helping ? "ok" : "wait"}`}>
        {helping ? "Help is on the way" : "Flare sent"}
      </p>
      <h1>
        {helping
          ? `The ${flare.claimed_by} assistant is helping you`
          : flare.position === 1
            ? "You're next in line"
            : `You're #${flare.position} in line`}
      </h1>
      {flare.question ? (
        <p className="your-question">“{flare.question}”</p>
      ) : (
        <p className="help">No question typed. The assistant will ask you.</p>
      )}
      {!helping && (
        <button className="btn-secondary" onClick={onCancel}>
          Cancel my Flare
        </button>
      )}
    </div>
  );
}

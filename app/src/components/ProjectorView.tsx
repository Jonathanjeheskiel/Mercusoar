import { useEffect, useRef } from "react";
import Logo from "./Logo";

/**
 * "Explain at the front": a full-window view for the projector.
 * It shows ONLY the question text, never the PC number, so the class
 * learns from the question without knowing who asked it.
 */
export default function ProjectorView({ question, onClose }: { question: string; onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="projector" role="dialog" aria-modal="true" aria-labelledby="projector-label">
      <p id="projector-label" className="projector-label">
        <Logo size={28} /> A question from the class
      </p>
      <p className="projector-text">{question}</p>
      <button ref={closeRef} className="btn-secondary" onClick={onClose}>
        Close (Esc)
      </button>
    </div>
  );
}

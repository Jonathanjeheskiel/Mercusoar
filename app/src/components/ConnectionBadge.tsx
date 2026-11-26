import { ConnectionStatus } from "../useHub";

const LABELS: Record<ConnectionStatus, string> = {
  connecting: "Connecting…",
  connected: "Connected",
  disconnected: "Hub unreachable, retrying…",
  rejected: "Wrong passcode",
};

/** Small pill showing whether this window is connected to the hub. */
export default function ConnectionBadge({ status }: { status: ConnectionStatus }) {
  return (
    <span className={`conn conn-${status}`} role="status">
      <i className="conn-dot" aria-hidden="true" />
      {LABELS[status]}
    </span>
  );
}

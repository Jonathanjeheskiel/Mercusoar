import { Flare } from "../protocol";

// Room layout for the prototype: 40 PCs in 5 rows of 8, row 1 at the front.
// PC 1 (front) and PC 40 (back) are the admin PCs.
// Planned: read the layout from each room's config file instead.
const COLUMNS = 8;
const TOTAL_PCS = 40;
const ADMIN_PCS: Record<number, string> = { 1: "Front", 40: "Back" };

type Props = {
  flares: Flare[];
  online: number[];
  onSelect: (flareId: number) => void;
};

export default function LabChart({ flares, online, onSelect }: Props) {
  const flareByPc = new Map(flares.map((f, i) => [f.pc, { flare: f, position: i + 1 }]));
  const onlineSet = new Set(online);

  return (
    <div>
      <p className="front-label">Front of the room</p>
      <div className="lab-chart" style={{ gridTemplateColumns: `repeat(${COLUMNS}, 1fr)` }}>
        {Array.from({ length: TOTAL_PCS }, (_, i) => i + 1).map((pc) => {
          const admin = ADMIN_PCS[pc];
          const entry = flareByPc.get(pc);

          if (admin) {
            return (
              <div key={pc} className="seat seat-admin" title={`PC ${pc}: ${admin} admin PC`}>
                <span className="seat-number">{pc}</span>
                <span className="seat-tag">{admin}</span>
              </div>
            );
          }

          if (entry) {
            const claimed = entry.flare.claimed_by !== null;
            const label = claimed
              ? `PC ${pc}: being helped by ${entry.flare.claimed_by}`
              : `PC ${pc}: Flare, number ${entry.position} in queue`;
            return (
              <button
                key={pc}
                className={`seat ${claimed ? "seat-claimed" : "seat-flare"}`}
                title={label}
                aria-label={label}
                onClick={() => onSelect(entry.flare.id)}
              >
                <span className="seat-number">{pc}</span>
                <span className="seat-tag">{claimed ? entry.flare.claimed_by : `#${entry.position}`}</span>
              </button>
            );
          }

          const isOnline = onlineSet.has(pc);
          return (
            <div
              key={pc}
              className={`seat ${isOnline ? "seat-online" : "seat-offline"}`}
              title={`PC ${pc}: ${isOnline ? "online" : "offline"}`}
            >
              <span className="seat-number">{pc}</span>
            </div>
          );
        })}
      </div>
      <div className="legend" aria-hidden="true">
        <span><i className="dot dot-offline" />Offline</span>
        <span><i className="dot dot-online" />Online</span>
        <span><i className="dot dot-flare" />Flare</span>
        <span><i className="dot dot-claimed" />Being helped</span>
        <span><i className="dot dot-admin" />Admin PC</span>
      </div>
    </div>
  );
}

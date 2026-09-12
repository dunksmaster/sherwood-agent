import type { Health } from "../api.ts";

function uptime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s}s` : `${s}s`;
}

export function StatusBar({ health }: { health: Health | null }) {
  const mode = health?.mode ?? "paper";
  return (
    <div className="topbar" data-mode={mode}>
      <div className="brand">
        <span className="brand-mark" aria-hidden="true" />
        <h1>
          sherwood <span className="dim">control plane</span>
        </h1>
      </div>
      <div className="row">
        <span className={`badge ${mode}`}>
          <span className="dot" aria-hidden="true" />
          {mode.toUpperCase()}
        </span>
        {health?.kill_switch && (
          <span className="badge kill">
            <span className="dot" aria-hidden="true" />
            KILL SWITCH ENGAGED
          </span>
        )}
        {health && (
          <span className="muted mono" title="server uptime">
            up {uptime(health.uptime_secs)}
          </span>
        )}
      </div>
    </div>
  );
}

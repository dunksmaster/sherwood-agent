import { useEffect } from "react";
import type { AuditEvent } from "../api.ts";

/**
 * Read `GET /v1/events` as Server-Sent Events. Uses `fetch` + a stream reader
 * rather than the browser `EventSource` because `EventSource` cannot send an
 * `Authorization` header. Each SSE frame carries a JSON array of new audit rows;
 * `onBatch` is called with the non-empty ones. Reconnects on remount.
 */
export function useAuditStream(
  token: string,
  onBatch: (rows: AuditEvent[]) => void,
  onUnauthorized: () => void,
): void {
  useEffect(() => {
    const ctrl = new AbortController();

    void (async () => {
      try {
        const resp = await fetch("/v1/events", {
          headers: { authorization: `Bearer ${token}` },
          signal: ctrl.signal,
        });
        if (resp.status === 401) {
          onUnauthorized();
          return;
        }
        if (!resp.ok || !resp.body) return;

        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let buf = "";

        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += decoder.decode(value, { stream: true });

          let sep: number;
          while ((sep = buf.indexOf("\n\n")) >= 0) {
            const frame = buf.slice(0, sep);
            buf = buf.slice(sep + 2);
            const data = frame
              .split("\n")
              .filter((l) => l.startsWith("data:"))
              .map((l) => l.slice(5).replace(/^ /, ""))
              .join("\n");
            if (!data) continue;
            try {
              const rows = JSON.parse(data) as AuditEvent[];
              if (Array.isArray(rows) && rows.length > 0) onBatch(rows);
            } catch {
              /* keep-alive comment or a partial frame — ignore */
            }
          }
        }
      } catch {
        /* aborted on unmount, or a network drop — the effect re-runs and reconnects */
      }
    })();

    return () => ctrl.abort();
  }, [token, onBatch, onUnauthorized]);
}

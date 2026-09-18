import type { TransportHealth } from "./types";

export function transportLevel(h: TransportHealth | null): "preparing" | "ok" | "warn" | "error" {
  if (!h) return "preparing";
  if (h.kind === "demo") return "ok";
  if (h.kind === "folder") return "warn";
  if (!h.running || !h.api_reachable) return "error";
  if (h.activity) return "preparing";
  if (h.peers === 0 || h.server_found === false) return "warn";
  return "ok";
}

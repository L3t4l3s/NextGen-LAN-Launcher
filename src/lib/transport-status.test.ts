import { describe, expect, it } from "vitest";
import { transportLevel } from "./transport-status";
import type { TransportHealth } from "./types";

const health: TransportHealth = {
  kind: "resilio", running: true, api_reachable: true, version: null,
  peers: 0, catalog_peers: 0, server_found: false, lan_mode: true,
  peer_details: false, detail: null, download_bps: 0, upload_bps: 0, web_ui: null,
};
describe("transport status", () => {
  it("shows initial startup and discovery without warning", () => {
    expect(transportLevel(null)).toBe("preparing");
    expect(transportLevel({ ...health, activity: "discovering" })).toBe("preparing");
    expect(transportLevel({ ...health, activity: "indexing" })).toBe("preparing");
  });
  it("retains real failures and warns after discovery grace expires", () => {
    expect(transportLevel(health)).toBe("warn");
    expect(transportLevel({ ...health, running: false, activity: "indexing" })).toBe("error");
    expect(transportLevel({ ...health, api_reachable: false, activity: "discovering" })).toBe("error");
    expect(transportLevel({ ...health, peers: 2, server_found: true })).toBe("ok");
  });
});

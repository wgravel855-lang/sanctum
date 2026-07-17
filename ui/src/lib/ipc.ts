// Data layer. Talks to the LocalSystem service through Tauri commands when
// running in the desktop app; falls back to in-memory mock data when opened
// in a plain browser (so the UI can be developed and previewed standalone).

import { invoke } from "@tauri-apps/api/core";
import type { Command, EventDto, Response, Status } from "./types";

export function inTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

// ---- mock state (browser preview only) ------------------------------------

const mockStatus: Status = {
  protection_active: true,
  degraded: false,
  total_blocked: 14382,
  protected_days: 27,
  streak: 12,
  locked: false,
  locked_until: null,
  schedule: { mode: "always_on" },
  blocklist_count: 61240,
  has_password: false,
  all_browsers: true,
};

let mockEvents: EventDto[] = [
  { ts: new Date(Date.now() - 3_600_000).toISOString(), kind: "block", detail: "blocked a request [dns]" },
  { ts: new Date(Date.now() - 7_200_000).toISOString(), kind: "block", detail: "blocked a request [hosts]" },
  { ts: new Date(Date.now() - 86_400_000).toISOString(), kind: "lock_start", detail: "480 min" },
];
let mockPassword: string | null = null;

function gate(password: string): boolean {
  return mockPassword === null || mockPassword === password;
}

async function mockCommand(cmd: Command): Promise<Response> {
  switch (cmd.cmd) {
    case "get_status":
      return { resp: "status", body: mockStatus };
    case "recent_events":
      return { resp: "events", body: mockEvents.slice(0, cmd.limit) };
    case "delete_history":
      mockEvents = [];
      return { resp: "deleted", body: { count: 3 } };
    case "add_block":
      mockStatus.blocklist_count += 1;
      return { resp: "ok" };
    case "start_lock":
      mockStatus.locked = true;
      mockStatus.locked_until = new Date(Date.now() + cmd.minutes * 60_000).toISOString();
      return { resp: "ok" };
    case "set_schedule":
      if (mockStatus.locked) return { resp: "denied", body: { reason: "Settings are frozen during a locked session." } };
      if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      mockStatus.schedule = cmd.schedule;
      return { resp: "ok" };
    case "set_password":
      if (mockPassword !== null && cmd.current !== mockPassword)
        return { resp: "denied", body: { reason: "Incorrect current password." } };
      mockPassword = cmd.new;
      mockStatus.has_password = true;
      return { resp: "ok" };
    case "verify_password":
      return gate(cmd.password) ? { resp: "ok" } : { resp: "denied", body: { reason: "Incorrect password." } };
    case "disable_protection":
      if (mockStatus.locked) return { resp: "denied", body: { reason: "Can't disable during a locked session." } };
      if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      mockStatus.protection_active = false;
      return { resp: "ok" };
    case "enable_protection":
      mockStatus.protection_active = true;
      return { resp: "ok" };
    default:
      return { resp: "ok" };
  }
}

// ---- public API -----------------------------------------------------------

export async function getStatus(): Promise<Status> {
  if (inTauri()) return invoke<Status>("get_status");
  return structuredClone(mockStatus);
}

export async function sendCommand(cmd: Command): Promise<Response> {
  if (inTauri()) return invoke<Response>("send_command", { command: cmd });
  return mockCommand(cmd);
}

export async function recentEvents(limit = 50): Promise<EventDto[]> {
  const r = await sendCommand({ cmd: "recent_events", limit });
  return r.resp === "events" ? r.body : [];
}

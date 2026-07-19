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
  blocking_now: true,
  degraded: false,
  total_blocked: 14382,
  urges_resisted: 34,
  protected_days: 27,
  streak: 12,
  locked: false,
  locked_until: null,
  schedule: { mode: "always_on" },
  blocklist_count: 47621,
  custom_block_count: 2,
  block_bypass: true,
  block_strict: false,
  uninstall_cooldown_hours: 0,
  accountability_on: false,
  accountability_sms_on: false,
  has_password: false,
  all_browsers: true,
};

let mockEvents: EventDto[] = (() => {
  const evs: EventDto[] = [];
  const now = Date.now();
  const push = (agoMs: number, kind: string) =>
    evs.push({ ts: new Date(now - agoMs).toISOString(), kind, detail: "" });
  [214, 168, 96, 141].forEach((count, day) => {
    for (let i = 0; i < count; i++) push(day * 86_400_000 + i * 30_000, "block");
  });
  push(2 * 86_400_000 + 100, "lock_start");
  return evs;
})();
let mockPassword: string | null = null;
let mockAccountabilityWebhook = "";
let mockSms = { sid: "", token: "", from: "", to: "" };
let mockCustomBlocks: string[] = ["distracting-site.net", "one-more-thing.com"];
let mockLetter: string | null =
  "Remember why you started. The version of you that set this up was thinking clearly and wanted better for you. This feeling passes. You are not missing anything real.";

function gate(password: string): boolean {
  return mockPassword === null || mockPassword === password;
}

async function mockCommand(cmd: Command): Promise<Response> {
  switch (cmd.cmd) {
    case "get_status":
      return { resp: "status", body: mockStatus };
    case "recent_events":
      return { resp: "events", body: mockEvents.slice(0, cmd.limit) };
    case "delete_history": {
      const count = mockEvents.length;
      mockEvents = [];
      return { resp: "deleted", body: { count } };
    }
    case "list_custom_blocks":
      return { resp: "custom_blocks", body: [...mockCustomBlocks] };
    case "add_block":
      if (!mockCustomBlocks.includes(cmd.domain)) {
        mockCustomBlocks.push(cmd.domain);
        mockStatus.blocklist_count += 1;
        mockStatus.custom_block_count = mockCustomBlocks.length;
      }
      return { resp: "ok" };
    case "remove_block":
      if (mockStatus.locked)
        return { resp: "denied", body: { reason: "The block list can only grow during a locked session." } };
      if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      if (mockCustomBlocks.includes(cmd.domain)) {
        mockCustomBlocks = mockCustomBlocks.filter((d) => d !== cmd.domain);
        mockStatus.blocklist_count -= 1;
        mockStatus.custom_block_count = mockCustomBlocks.length;
      }
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
    case "set_bypass_blocking":
      if (!cmd.enabled) {
        if (mockStatus.locked)
          return { resp: "denied", body: { reason: "Can't turn off during a locked session." } };
        if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      }
      mockStatus.block_bypass = cmd.enabled;
      return { resp: "ok" };
    case "set_strict_mode":
      if (!cmd.enabled) {
        if (mockStatus.locked)
          return { resp: "denied", body: { reason: "Can't turn off during a locked session." } };
        if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      }
      mockStatus.block_strict = cmd.enabled;
      return { resp: "ok" };
    case "set_uninstall_cooldown": {
      const cur = mockStatus.uninstall_cooldown_hours;
      const want = Math.min(cmd.hours, 720);
      if (cur > 0 && want < cur)
        return { resp: "denied", body: { reason: "The uninstall cooldown can only be increased, not reduced or turned off." } };
      mockStatus.uninstall_cooldown_hours = want;
      return { resp: "ok" };
    }
    case "set_accountability": {
      const old = mockAccountabilityWebhook.trim();
      const next = cmd.webhook.trim();
      if (old && next !== old) {
        if (mockStatus.locked)
          return { resp: "denied", body: { reason: "The accountability partner is frozen during a locked session." } };
        if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      }
      mockAccountabilityWebhook = next;
      mockStatus.accountability_on = next.length > 0;
      return { resp: "ok" };
    }
    case "set_accountability_sms": {
      const had = mockSms.sid && mockSms.token && mockSms.from && mockSms.to;
      const complete = cmd.sid.trim() && cmd.token.trim() && cmd.from.trim() && cmd.to.trim();
      if (had) {
        if (mockStatus.locked)
          return { resp: "denied", body: { reason: "The accountability partner is frozen during a locked session." } };
        if (!gate(cmd.password)) return { resp: "denied", body: { reason: "Incorrect password." } };
      }
      mockSms = { sid: cmd.sid.trim(), token: cmd.token.trim(), from: cmd.from.trim(), to: cmd.to.trim() };
      mockStatus.accountability_sms_on = !!complete;
      return { resp: "ok" };
    }
    case "test_accountability":
      return mockAccountabilityWebhook.trim() || mockStatus.accountability_sms_on
        ? { resp: "ok" }
        : { resp: "denied", body: { reason: "No accountability partner is set." } };
    case "resolve_intervention":
      mockStatus.urges_resisted += 1;
      return { resp: "ok" };
    case "get_letter":
      return { resp: "letter", body: mockLetter };
    case "set_letter":
      mockLetter = cmd.text.trim() || null;
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

export async function listCustomBlocks(): Promise<string[]> {
  const r = await sendCommand({ cmd: "list_custom_blocks" });
  return r.resp === "custom_blocks" ? r.body : [];
}

export async function getLetter(): Promise<string | null> {
  const r = await sendCommand({ cmd: "get_letter" });
  return r.resp === "letter" ? r.body : null;
}

export async function setLetter(text: string): Promise<Response> {
  return sendCommand({ cmd: "set_letter", text });
}

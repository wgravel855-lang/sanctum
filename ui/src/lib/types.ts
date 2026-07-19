// Mirrors sanctum-core::ipc (serde). Response is adjacently tagged
// (`resp` + `body`); Command is internally tagged (`cmd`).

export type TimeWindow = {
  start_min: number;
  end_min: number;
  days: number[];
};

export type Schedule =
  | { mode: "always_on" }
  | { mode: "off" }
  | { mode: "windows"; windows: TimeWindow[] }
  | { mode: "focus"; ends_at: string };

export interface Status {
  protection_active: boolean;
  blocking_now: boolean;
  degraded: boolean;
  total_blocked: number;
  urges_resisted: number;
  protected_days: number;
  streak: number;
  locked: boolean;
  locked_until: string | null;
  schedule: Schedule;
  blocklist_count: number;
  custom_block_count: number;
  block_bypass: boolean;
  block_strict: boolean;
  uninstall_cooldown_hours: number;
  has_password: boolean;
  all_browsers: boolean;
}

export interface EventDto {
  ts: string;
  kind: string;
  detail: string;
}

export type Command =
  | { cmd: "get_status" }
  | { cmd: "recent_events"; limit: number }
  | { cmd: "list_custom_blocks" }
  | { cmd: "add_block"; domain: string }
  | { cmd: "remove_block"; domain: string; password: string }
  | { cmd: "add_allow"; domain: string; password: string }
  | { cmd: "remove_allow"; domain: string }
  | { cmd: "set_schedule"; schedule: Schedule; password: string }
  | { cmd: "start_lock"; minutes: number }
  | { cmd: "extend_lock"; minutes: number }
  | { cmd: "set_password"; new: string; current: string | null }
  | { cmd: "verify_password"; password: string }
  | { cmd: "disable_protection"; password: string }
  | { cmd: "enable_protection" }
  | { cmd: "set_bypass_blocking"; enabled: boolean; password: string }
  | { cmd: "set_strict_mode"; enabled: boolean; password: string }
  | { cmd: "set_uninstall_cooldown"; hours: number }
  | { cmd: "delete_history" }
  | { cmd: "poll_intervention" }
  | { cmd: "trigger_intervention" }
  | { cmd: "resolve_intervention" }
  | { cmd: "get_letter" }
  | { cmd: "set_letter"; text: string };

export interface InterventionDto {
  pending: boolean;
  domain: string | null;
  urges_while_away: number;
  letter?: string | null;
}

export type Response =
  | { resp: "status"; body: Status }
  | { resp: "events"; body: EventDto[] }
  | { resp: "custom_blocks"; body: string[] }
  | { resp: "deleted"; body: { count: number } }
  | { resp: "ok" }
  | { resp: "denied"; body: { reason: string } }
  | { resp: "error"; body: { message: string } }
  | { resp: "intervention"; body: InterventionDto }
  | { resp: "letter"; body: string | null };

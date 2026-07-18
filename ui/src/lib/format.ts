import type { EventDto } from "./types";

export function commas(n: number): string {
  return n.toLocaleString("en-US");
}

export interface ActivityItem {
  text: string;
  when: string;
}

function dayLabel(d: Date): string {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const that = new Date(d);
  that.setHours(0, 0, 0, 0);
  const diff = Math.round((today.getTime() - that.getTime()) / 86_400_000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function humanEvent(kind: string): string {
  switch (kind) {
    case "lock_start":
      return "Started a locked session";
    case "lock_extend":
      return "Extended the locked session";
    case "protection_disabled":
      return "Turned protection off";
    case "protection_enabled":
      return "Turned protection on";
    case "block_add":
      return "Added a site to the block list";
    default:
      return kind.replace(/_/g, " ").replace(/^\w/, (c) => c.toUpperCase());
  }
}

/** Turn raw event rows into human sentences, aggregating blocks per day.
 *  e.g. "Blocked 214 attempts" · "Today". Never surfaces raw log strings. */
export function aggregateActivity(events: EventDto[]): ActivityItem[] {
  const blocksByDay = new Map<string, { count: number; ts: number }>();
  const items: (ActivityItem & { ts: number })[] = [];

  for (const e of events) {
    const d = new Date(e.ts);
    if (e.kind === "block") {
      const key = d.toDateString();
      const cur = blocksByDay.get(key) ?? { count: 0, ts: d.getTime() };
      cur.count += 1;
      cur.ts = Math.max(cur.ts, d.getTime());
      blocksByDay.set(key, cur);
    } else {
      items.push({ text: humanEvent(e.kind), when: dayLabel(d), ts: d.getTime() });
    }
  }

  for (const [key, v] of blocksByDay) {
    items.push({
      text: `Blocked ${commas(v.count)} attempt${v.count === 1 ? "" : "s"}`,
      when: dayLabel(new Date(key)),
      ts: v.ts,
    });
  }

  return items
    .sort((a, b) => b.ts - a.ts)
    .map(({ text, when }) => ({ text, when }));
}

/** A calm, human relative time like "in 3 days" / "in 5 hours". */
export function untilHuman(iso: string | null): string {
  if (!iso) return "";
  const ms = new Date(iso).getTime() - Date.now();
  if (ms <= 0) return "any moment";
  const mins = Math.round(ms / 60_000);
  if (mins < 60) return `${mins} min`;
  const hours = Math.round(mins / 60);
  if (hours < 48) return `${hours} hour${hours === 1 ? "" : "s"}`;
  const days = Math.round(hours / 24);
  return `${days} day${days === 1 ? "" : "s"}`;
}

/** Absolute local date-time, e.g. "Mon, Jul 20, 6:00 AM". */
export function dateTimeHuman(iso: string | null): string {
  if (!iso) return "";
  return new Date(iso).toLocaleString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

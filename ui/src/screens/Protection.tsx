import { useState } from "react";
import type { Response, Status } from "../lib/types";
import { dateTimeHuman, untilHuman } from "../lib/format";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Switch from "../components/Switch";
import Button from "../components/Button";
import SecuritySection from "../components/SecuritySection";
import ConfirmModal from "../components/ConfirmModal";
import { Group, GroupFootnote, Row } from "../components/List";

const DURATIONS = [
  { label: "1 hour", minutes: 60 },
  { label: "Tonight · 8h", minutes: 480 },
  { label: "1 day", minutes: 1440 },
  { label: "1 week", minutes: 10080 },
];

// Shown before the user commits to each mode, so nothing is a surprise.
// Honest about the limit: Sanctum sees hostnames, not pages. Saying otherwise
// would be the kind of overclaiming this app exists to avoid.
const KEYWORDS_COPY = {
  title: "Block by keyword?",
  body: "Sanctum will block any site whose web address contains an adult word, even if it isn't on the block list yet. That catches new sites a fixed list can't keep up with.\n\nIt reads the address only, not the page. Sanctum can't see inside a site, because that would mean decrypting your traffic, and it will never do that.\n\nShort words are matched whole, so essex.ac.uk and analytics.google.com keep working. If something you need does get caught, you can allow it.",
};

const STRICT_COPY = {
  title: "Turn on Strict mode?",
  body: "Strict mode also blocks mainstream social and image-heavy sites like Instagram, Pinterest, Reddit, and TikTok, which often surface suggestive content. Those sites will stop loading in every browser on this computer.\n\nYou can turn Strict mode back off with your settings password, unless a locked session is running.",
};
const COLD_COPY = {
  title: "Start a Cold Turkey session?",
  body: "For the length of time you pick, every setting is frozen. You can add sites to the block list but cannot remove any, protection cannot be turned off, and your password will not unlock it.\n\nThe only early way out is restarting Windows in Safe Mode, so this is meant to outlast a craving rather than trap you.",
};

const COOLDOWN_OPTIONS = [
  { label: "12 hours", h: 12 },
  { label: "24 hours", h: 24 },
  { label: "2 days", h: 48 },
  { label: "1 week", h: 168 },
];
function cooldownLabel(h: number): string {
  if (h <= 0) return "Off";
  if (h < 24) return `${h}h`;
  if (h % 168 === 0) return `${h / 168}w`;
  if (h % 24 === 0) return `${h / 24}d`;
  return `${h}h`;
}

export default function Protection({
  status,
  onBack,
  refresh,
  onOpenAccountability,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
  onOpenAccountability: () => void;
}) {
  const locked = !!status?.locked;
  const active = !!status?.protection_active;
  const degraded = !!status?.degraded;
  const hasPassword = !!status?.has_password;

  const [busy, setBusy] = useState(false);
  const [armCT, setArmCT] = useState(false);
  const [pwPrompt, setPwPrompt] = useState(false);
  const [password, setPassword] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [bypassPrompt, setBypassPrompt] = useState(false);
  const [bypassPw, setBypassPw] = useState("");
  const [strictPrompt, setStrictPrompt] = useState(false);
  const [strictPw, setStrictPw] = useState("");
  const [kwPrompt, setKwPrompt] = useState(false);
  const [kwPw, setKwPw] = useState("");
  const [modal, setModal] = useState<null | "strict" | "cold" | "keywords">(null);
  const [cooldownPicker, setCooldownPicker] = useState(false);
  const [pendingCooldown, setPendingCooldown] = useState<number | null>(null);

  const bypassOn = status?.block_bypass ?? true;
  const strictOn = status?.block_strict ?? false;
  const keywordsOn = status?.block_keywords ?? false;
  const cooldownHours = status?.uninstall_cooldown_hours ?? 0;

  const handle = async (r: Response, okMsg?: string) => {
    if (r.resp === "ok") setNote(okMsg ?? null);
    else if (r.resp === "denied") setNote(r.body.reason);
    else if (r.resp === "error") setNote(r.body.message);
    await refresh();
  };

  const toggleProtection = async (next: boolean) => {
    setNote(null);
    if (next) {
      setBusy(true);
      await handle(await sendCommand({ cmd: "enable_protection" }), "Protection on.");
      setBusy(false);
    } else if (hasPassword) {
      setPwPrompt(true);
    } else {
      setBusy(true);
      await handle(await sendCommand({ cmd: "disable_protection", password: "" }), "Protection off.");
      setBusy(false);
    }
  };

  const confirmDisable = async () => {
    setBusy(true);
    await handle(await sendCommand({ cmd: "disable_protection", password }), "Protection off.");
    setPassword("");
    setPwPrompt(false);
    setBusy(false);
  };

  const toggleBypass = async (next: boolean) => {
    setNote(null);
    if (next) {
      setBusy(true);
      await handle(await sendCommand({ cmd: "set_bypass_blocking", enabled: true, password: "" }), "Bypass blocking on.");
      setBusy(false);
    } else if (hasPassword) {
      setBypassPrompt(true);
    } else {
      setBusy(true);
      await handle(await sendCommand({ cmd: "set_bypass_blocking", enabled: false, password: "" }), "Bypass blocking off.");
      setBusy(false);
    }
  };

  const confirmBypassOff = async () => {
    setBusy(true);
    await handle(await sendCommand({ cmd: "set_bypass_blocking", enabled: false, password: bypassPw }), "Bypass blocking off.");
    setBypassPw("");
    setBypassPrompt(false);
    setBusy(false);
  };

  const setStrict = async (enabled: boolean, pw: string) => {
    setBusy(true);
    await handle(
      await sendCommand({ cmd: "set_strict_mode", enabled, password: pw }),
      enabled ? "Strict mode on." : "Strict mode off.",
    );
    setBusy(false);
  };

  const toggleStrict = (next: boolean) => {
    setNote(null);
    if (next) {
      setModal("strict"); // confirm before enabling
    } else if (hasPassword) {
      setStrictPrompt(true);
    } else {
      setStrict(false, "");
    }
  };

  const confirmStrictOff = async () => {
    await setStrict(false, strictPw);
    setStrictPw("");
    setStrictPrompt(false);
  };

  const setKeywords = async (enabled: boolean, pw: string) => {
    setBusy(true);
    await handle(
      await sendCommand({ cmd: "set_keyword_blocking", enabled, password: pw }),
      enabled ? "Keyword blocking on." : "Keyword blocking off.",
    );
    setBusy(false);
  };

  const toggleKeywords = (next: boolean) => {
    setNote(null);
    if (next) {
      setModal("keywords"); // confirm first: explain what it can and can't do
    } else if (hasPassword) {
      setKwPrompt(true);
    } else {
      setKeywords(false, "");
    }
  };

  const confirmKeywordsOff = async () => {
    await setKeywords(false, kwPw);
    setKwPw("");
    setKwPrompt(false);
  };

  const toggleColdTurkey = (next: boolean) => {
    setNote(null);
    if (next) setModal("cold"); // confirm before arming
    else setArmCT(false);
  };

  const setCooldown = async (hours: number) => {
    setBusy(true);
    await handle(
      await sendCommand({ cmd: "set_uninstall_cooldown", hours }),
      "Uninstall cooldown set.",
    );
    setBusy(false);
  };

  const startLock = async (minutes: number) => {
    setBusy(true);
    setNote(null);
    if (!active) await sendCommand({ cmd: "enable_protection" });
    await handle(await sendCommand({ cmd: "start_lock", minutes }));
    setArmCT(false);
    setBusy(false);
  };

  const protectionSubtitle = locked ? "Locked on" : degraded ? "Degraded, HOSTS-only" : active ? "Active" : "Off";

  return (
    <div className="screen">
      <TopBar title="Protection" onBack={onBack} />

      <Group>
        <Row>
          <span className="flex flex-col">
            <span className="t-row-title">Protection</span>
            <span className="t-caption">{protectionSubtitle}</span>
          </span>
          <span className="row-trailing">
            <Switch checked={active || locked} disabled={locked || busy} onChange={toggleProtection} label="Protection" />
          </span>
        </Row>
        <Row>
          <span className="t-row-title">Coverage</span>
          <span className="row-trailing t-subtitle">All browsers · DNS + hosts</span>
        </Row>
        <Row>
          <span className="flex flex-col">
            <span className="t-row-title">Block bypass tools</span>
            <span className="t-caption">Proxies, VPNs, Tor, DoH</span>
          </span>
          <span className="row-trailing">
            <Switch
              checked={bypassOn}
              disabled={locked || busy}
              onChange={toggleBypass}
              label="Block bypass tools"
            />
          </span>
        </Row>
        <Row>
          <span className="flex flex-col">
            <span className="t-row-title">Strict mode</span>
            <span className="t-caption">Also block social & image sites</span>
          </span>
          <span className="row-trailing">
            <Switch
              checked={strictOn}
              disabled={locked || busy}
              onChange={toggleStrict}
              label="Strict mode"
            />
          </span>
        </Row>
        <Row>
          <span className="flex flex-col">
            <span className="t-row-title">Block by keyword</span>
            <span className="t-caption">Catch sites whose name gives them away</span>
          </span>
          <span className="row-trailing">
            <Switch
              checked={keywordsOn}
              disabled={locked || busy}
              onChange={toggleKeywords}
              label="Block by keyword"
            />
          </span>
        </Row>
      </Group>

      {strictPrompt && (
        <div className="mt-3">
          <p className="t-caption mb-2 px-4">Enter your password to turn Strict mode off.</p>
          <input
            type="password"
            autoFocus
            value={strictPw}
            onChange={(e) => setStrictPw(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && confirmStrictOff()}
            placeholder="Password"
            className="field"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => {
                setStrictPrompt(false);
                setStrictPw("");
              }}
              className="pressable text-[15px] text-text-2"
            >
              Cancel
            </button>
            <Button variant="destructive" onClick={confirmStrictOff}>
              Turn off
            </Button>
          </div>
        </div>
      )}

      {kwPrompt && (
        <div className="mt-3">
          <p className="t-caption mb-2 px-4">Enter your password to turn keyword blocking off.</p>
          <input
            type="password"
            autoFocus
            value={kwPw}
            onChange={(e) => setKwPw(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && confirmKeywordsOff()}
            placeholder="Password"
            className="field"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => {
                setKwPrompt(false);
                setKwPw("");
              }}
              className="pressable text-[15px] text-text-2"
            >
              Cancel
            </button>
            <Button variant="destructive" onClick={confirmKeywordsOff}>
              Turn off
            </Button>
          </div>
        </div>
      )}

      {bypassPrompt && (
        <div className="mt-3">
          <p className="t-caption mb-2 px-4">
            Enter your password to stop blocking bypass tools.
          </p>
          <input
            type="password"
            autoFocus
            value={bypassPw}
            onChange={(e) => setBypassPw(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && confirmBypassOff()}
            placeholder="Password"
            className="field"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => {
                setBypassPrompt(false);
                setBypassPw("");
              }}
              className="pressable text-[15px] text-text-2"
            >
              Cancel
            </button>
            <Button variant="destructive" onClick={confirmBypassOff}>
              Turn off
            </Button>
          </div>
        </div>
      )}

      {pwPrompt && (
        <div className="mt-3">
          <p className="t-caption mb-2 px-4">Enter your password to turn protection off.</p>
          <input
            type="password"
            autoFocus
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && confirmDisable()}
            placeholder="Password"
            className="field"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => {
                setPwPrompt(false);
                setPassword("");
              }}
              className="pressable text-[15px] text-text-2"
            >
              Cancel
            </button>
            <Button variant="destructive" onClick={confirmDisable}>
              Turn protection off
            </Button>
          </div>
        </div>
      )}

      {/* Cold Turkey */}
      <div className="mt-8">
        <Group>
          <Row>
            <span className="flex flex-col">
              <span className="t-row-title">Cold Turkey mode</span>
              <span className="t-caption">
                {locked ? `Locked · ${untilHuman(status?.locked_until ?? null)} left` : "Off"}
              </span>
            </span>
            <span className="row-trailing">
              <Switch
                checked={locked || armCT}
                disabled={locked || busy}
                onChange={toggleColdTurkey}
                label="Cold Turkey mode"
              />
            </span>
          </Row>
        </Group>

        {armCT && !locked && (
          <div className="mt-3">
            <p className="t-caption mb-3 px-1">
              Choose how long to lock. While locked, settings freeze and the block list can only
              grow. <span className="text-text-1">You won't be able to turn this off early.</span>
            </p>
            <div className="grid grid-cols-2 gap-2">
              {DURATIONS.map((d) => (
                <button
                  key={d.minutes}
                  disabled={busy}
                  onClick={() => startLock(d.minutes)}
                  className="pressable rounded-[10px] border border-hairline py-3 text-[15px] text-text-1 disabled:opacity-50"
                >
                  Lock {d.label}
                </button>
              ))}
            </div>
          </div>
        )}

        {locked && (
          <GroupFootnote>
            Locked until {dateTimeHuman(status?.locked_until ?? null)}. This can't be turned off
            early from inside the app. Removing it before then requires booting Windows into Safe
            Mode. That friction is the point. It's meant to outlast a craving, not to be impossible.
          </GroupFootnote>
        )}
      </div>

      {/* Accountability partner. */}
      <div className="mt-8">
        <Group>
          <Row onClick={onOpenAccountability}>
            <span className="flex flex-col">
              <span className="t-row-title">Accountability</span>
              <span className="t-caption">Alert a trusted person if protection is weakened</span>
            </span>
            <span className="row-trailing t-subtitle">
              {status?.accountability_on || status?.accountability_sms_on ? "On" : "Off"} ›
            </span>
          </Row>
        </Group>
      </div>

      {/* Opt-in uninstall cooldown — binding once set (grow-only). */}
      <div className="mt-8">
        <Group>
          <Row onClick={() => setCooldownPicker((v) => !v)}>
            <span className="flex flex-col">
              <span className="t-row-title">Uninstall cooldown</span>
              <span className="t-caption">
                {cooldownHours > 0 ? "Set · can only be increased" : "Off"}
              </span>
            </span>
            <span className="row-trailing t-subtitle">{cooldownLabel(cooldownHours)} ›</span>
          </Row>
        </Group>

        {cooldownPicker && (
          <div className="mt-3">
            <p className="t-caption mb-3 px-1">
              Make uninstalling Sanctum wait a set time.{" "}
              <span className="text-text-1">
                Once you set it you can't reduce or remove it, only increase it.
              </span>{" "}
              Rebooting into Safe Mode always removes Sanctum right away.
            </p>
            {COOLDOWN_OPTIONS.filter((o) => o.h > cooldownHours).length > 0 ? (
              <div className="grid grid-cols-2 gap-2">
                {COOLDOWN_OPTIONS.filter((o) => o.h > cooldownHours).map((o) => (
                  <button
                    key={o.h}
                    disabled={busy}
                    onClick={() => setPendingCooldown(o.h)}
                    className="pressable rounded-[10px] border border-hairline py-3 text-[15px] text-text-1 disabled:opacity-50"
                  >
                    Set {o.label}
                  </button>
                ))}
              </div>
            ) : (
              <p className="t-caption px-1">This is already the maximum.</p>
            )}
          </div>
        )}
      </div>

      {note && <p className="t-caption mt-4 text-center">{note}</p>}

      <SecuritySection status={status} refresh={refresh} />

      <ConfirmModal
        open={modal === "strict"}
        title={STRICT_COPY.title}
        body={STRICT_COPY.body}
        confirmLabel="Turn on Strict mode"
        onConfirm={() => {
          setModal(null);
          setStrict(true, "");
        }}
        onCancel={() => setModal(null)}
      />
      <ConfirmModal
        open={modal === "keywords"}
        title={KEYWORDS_COPY.title}
        body={KEYWORDS_COPY.body}
        confirmLabel="Turn on keyword blocking"
        onConfirm={() => {
          setModal(null);
          setKeywords(true, "");
        }}
        onCancel={() => setModal(null)}
      />
      <ConfirmModal
        open={modal === "cold"}
        title={COLD_COPY.title}
        body={COLD_COPY.body}
        confirmLabel="I understand, continue"
        onConfirm={() => {
          setModal(null);
          setArmCT(true);
        }}
        onCancel={() => setModal(null)}
      />
      <ConfirmModal
        open={pendingCooldown !== null}
        title="Set this uninstall cooldown?"
        body={`Uninstalling Sanctum will then wait ${
          COOLDOWN_OPTIONS.find((o) => o.h === pendingCooldown)?.label ??
          `${pendingCooldown} hours`
        } before it finishes. You won't be able to reduce or remove this cooldown afterward, only increase it. Rebooting into Safe Mode still removes Sanctum immediately.`}
        confirmLabel="Set cooldown"
        onConfirm={() => {
          const h = pendingCooldown ?? 0;
          setPendingCooldown(null);
          setCooldownPicker(false);
          setCooldown(h);
        }}
        onCancel={() => setPendingCooldown(null)}
      />
    </div>
  );
}

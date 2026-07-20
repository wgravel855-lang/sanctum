import { useState } from "react";
import { QRCodeSVG } from "qrcode.react";
import type { Response, Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import Switch from "../components/Switch";
import ConfirmModal from "../components/ConfirmModal";
import { GroupLabel, GroupFootnote, Row } from "../components/List";

// Honest accountability. The one-tap default is ntfy: Sanctum generates a
// private topic, the partner installs the free ntfy app and scans a code — the
// user types nothing, and there's still no server or account of ours. Custom
// webhooks and Twilio SMS are under Advanced. Signals only, never content.

function genTopic(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  const chars = "abcdefghijklmnopqrstuvwxyz0123456789";
  let s = "";
  for (const b of bytes) s += chars[b % chars.length];
  return `sanctum-${s}`;
}

export default function Accountability({
  status,
  onBack,
  refresh,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const ntfyTopic = status?.accountability_ntfy_topic ?? null;
  const webhookOn = !!status?.accountability_on;
  const customWebhookOn = webhookOn && !ntfyTopic;
  const smsOn = !!status?.accountability_sms_on;
  const anyOn = webhookOn || smsOn;
  const heartbeatOn = status?.heartbeat_on ?? true;
  const locked = !!status?.locked;
  const hasPassword = !!status?.has_password;

  const [url, setUrl] = useState("");
  const [sid, setSid] = useState("");
  const [token, setToken] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [password, setPassword] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<null | "webhook" | "sms">(null);
  const [confirmHeartbeatOff, setConfirmHeartbeatOff] = useState(false);

  const run = async (cmd: Parameters<typeof sendCommand>[0], ok: string) => {
    setBusy(true);
    setNote(null);
    const r = (await sendCommand(cmd)) as Response;
    if (r.resp === "ok") setNote(ok);
    else if (r.resp === "denied") setNote(r.body.reason);
    else if (r.resp === "error") setNote(r.body.message);
    await refresh();
    setBusy(false);
  };

  const setupNtfy = () =>
    run(
      { cmd: "set_accountability", webhook: `https://ntfy.sh/${genTopic()}`, password },
      "Phone alerts are on. Have your partner subscribe with the code below.",
    );

  const test = () => run({ cmd: "test_accountability" }, "Test sent — ask them to confirm it arrived.");

  const onToggleHeartbeat = () => {
    if (heartbeatOn) {
      setConfirmHeartbeatOff(true); // turning it off reduces oversight — confirm
    } else {
      run({ cmd: "set_heartbeat", enabled: true, password }, "Weekly check-in turned on.");
    }
  };

  const smsComplete = sid.trim() && token.trim() && from.trim() && to.trim();

  return (
    <div className="screen">
      <TopBar title="Accountability" onBack={onBack} />

      <p className="t-body text-text-2">
        Let a trusted person know if you ever weaken your protection. They get a
        short alert the moment protection is turned off or an uninstall starts —
        never anything about what you browse.
      </p>

      {/* Hero: one-tap phone alerts via ntfy. */}
      <div className="mt-8">
        <GroupLabel>Phone alerts</GroupLabel>
        {ntfyTopic ? (
          <>
            <div className="rounded-[16px] border border-hairline bg-surface-1 p-5 text-center">
              <div className="mx-auto w-fit rounded-[12px] bg-white p-3">
                <QRCodeSVG value={`https://ntfy.sh/${ntfyTopic}`} size={168} bgColor="#ffffff" fgColor="#000000" level="M" />
              </div>
              <p className="t-body mt-4 text-text-1">Point your partner's phone here</p>
              <p className="t-caption mt-2">
                Have them install the free <span className="text-text-1">ntfy</span> app, tap +, and
                scan this — or add the topic below on server ntfy.sh.
              </p>
              <p className="t-caption mt-3 break-all text-text-2">ntfy.sh/{ntfyTopic}</p>
            </div>
            <div className="mt-4 flex flex-col gap-3">
              <Button variant="secondary" onClick={test} disabled={busy}>
                Send a test alert
              </Button>
              <div className="flex justify-center">
                <Button variant="destructive" onClick={() => setConfirmRemove("webhook")} disabled={busy || locked}>
                  Turn off phone alerts
                </Button>
              </div>
            </div>
          </>
        ) : (
          <>
            <p className="t-body text-text-2">
              One tap makes a private alert channel. Your partner just installs the free ntfy app and
              scans a code — no account, nothing for you to type.
            </p>
            <Button className="mt-3" onClick={setupNtfy} disabled={busy || customWebhookOn}>
              Set up phone alerts
            </Button>
            {customWebhookOn && (
              <GroupFootnote>A custom webhook is set (under Advanced). Remove it first to switch to phone alerts.</GroupFootnote>
            )}
          </>
        )}
      </div>

      {/* Weekly "still protected" heartbeat — its absence is the tamper signal. */}
      {anyOn && (
        <div className="mt-8">
          <GroupLabel>Weekly check-in</GroupLabel>
          <Row>
            <span className="flex flex-col pr-3">
              <span className="t-row-title">Send a weekly "still protected" note</span>
              <span className="t-caption">
                A short note each week that protection is still on. If it ever stops arriving, your
                partner knows to check in.
              </span>
            </span>
            <span className="row-trailing">
              <Switch
                checked={heartbeatOn}
                disabled={busy || locked}
                onChange={onToggleHeartbeat}
                label="Weekly check-in"
              />
            </span>
          </Row>
        </div>
      )}

      {/* Advanced: custom webhook + Twilio SMS. */}
      <div className="mt-8">
        <button
          onClick={() => setAdvanced((v) => !v)}
          className="pressable t-label mb-1 flex w-full items-center justify-between px-1"
        >
          <span>Advanced channels</span>
          <span className="text-text-3">{advanced ? "Hide" : "Show"}</span>
        </button>

        {advanced && (
          <div className="mt-2">
            {/* Custom webhook (Discord / Slack / any HTTPS). */}
            <GroupLabel>Chat app or custom webhook</GroupLabel>
            {customWebhookOn ? (
              <div className="flex justify-center">
                <Button variant="destructive" onClick={() => setConfirmRemove("webhook")} disabled={busy || locked}>
                  Remove webhook
                </Button>
              </div>
            ) : (
              <>
                <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="Webhook URL (Discord, Slack…)" spellCheck={false} autoCapitalize="none" className="field" />
                <Button
                  className="mt-3"
                  disabled={busy || !url.trim() || ntfyTopic !== null}
                  onClick={() => run({ cmd: "set_accountability", webhook: url.trim(), password }, "Webhook connected.").then(() => setUrl(""))}
                >
                  Connect webhook
                </Button>
                {ntfyTopic && <GroupFootnote>Phone alerts are on. Turn them off first to use a custom webhook instead.</GroupFootnote>}
              </>
            )}

            {/* Twilio SMS. */}
            <div className="mt-6">
              <GroupLabel>Text message (your Twilio)</GroupLabel>
              {smsOn ? (
                <div className="flex justify-center">
                  <Button variant="destructive" onClick={() => setConfirmRemove("sms")} disabled={busy || locked}>
                    Remove SMS
                  </Button>
                </div>
              ) : (
                <>
                  <input value={sid} onChange={(e) => setSid(e.target.value)} placeholder="Twilio Account SID (AC…)" spellCheck={false} autoCapitalize="none" className="field" />
                  <input value={token} onChange={(e) => setToken(e.target.value)} type="password" placeholder="Twilio Auth Token" className="field mt-3" />
                  <input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="Your Twilio number (+1…)" className="field mt-3" />
                  <input value={to} onChange={(e) => setTo(e.target.value)} placeholder="Partner's phone (+1…)" className="field mt-3" />
                  <Button
                    className="mt-3"
                    disabled={busy || !smsComplete}
                    onClick={() =>
                      run({ cmd: "set_accountability_sms", sid: sid.trim(), token: token.trim(), from: from.trim(), to: to.trim(), password: "" }, "SMS connected. Sent a hello text.").then(() => {
                        setSid(""); setToken(""); setFrom(""); setTo("");
                      })
                    }
                  >
                    Connect SMS
                  </Button>
                  <GroupFootnote>Needs your own Twilio account (~1¢/text). Auth token stored only on this PC.</GroupFootnote>
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Password for change/remove when one is set. */}
      {anyOn && hasPassword && (
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Your password (to change or remove)"
          className="field mt-6"
        />
      )}
      {anyOn && !hasPassword && (
        <GroupFootnote>Set a password on the Protection screen so a channel can't be removed on a whim.</GroupFootnote>
      )}

      {note && <p className="t-caption mt-6 text-center">{note}</p>}

      <ConfirmModal
        open={confirmRemove !== null}
        title={confirmRemove === "sms" ? "Remove SMS partner?" : "Turn off phone alerts?"}
        body="Your partner will be told this channel was removed, and it will stop sending. You can set one up again anytime."
        confirmLabel="Turn off"
        onConfirm={() => {
          const which = confirmRemove;
          setConfirmRemove(null);
          if (which === "sms") {
            run({ cmd: "set_accountability_sms", sid: "", token: "", from: "", to: "", password }, "SMS removed. Your partner was told.").then(() => setPassword(""));
          } else {
            run({ cmd: "set_accountability", webhook: "", password }, "Phone alerts off. Your partner was told.").then(() => setPassword(""));
          }
        }}
        onCancel={() => setConfirmRemove(null)}
      />

      <ConfirmModal
        open={confirmHeartbeatOff}
        title="Turn off weekly check-ins?"
        body="Your partner will stop getting the weekly note that protection is still on, and they'll be told it was turned off. You can turn it back on anytime."
        confirmLabel="Turn off"
        onConfirm={() => {
          setConfirmHeartbeatOff(false);
          run({ cmd: "set_heartbeat", enabled: false, password }, "Weekly check-in turned off. Your partner was told.").then(() => setPassword(""));
        }}
        onCancel={() => setConfirmHeartbeatOff(false)}
      />
    </div>
  );
}

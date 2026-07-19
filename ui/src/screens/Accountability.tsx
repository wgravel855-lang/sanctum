import { useState } from "react";
import type { Response, Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import ConfirmModal from "../components/ConfirmModal";
import { Group, GroupLabel, GroupFootnote, Row } from "../components/List";

// Honest accountability: a partner gets short protection-state signals through a
// channel the USER owns — a chat-app/push webhook OR their own Twilio SMS.
// Sanctum runs no server and never sends browsing content.

export default function Accountability({
  status,
  onBack,
  refresh,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const webhookOn = !!status?.accountability_on;
  const smsOn = !!status?.accountability_sms_on;
  const anyOn = webhookOn || smsOn;
  const locked = !!status?.locked;
  const hasPassword = !!status?.has_password;

  const [url, setUrl] = useState("");
  const [sid, setSid] = useState("");
  const [token, setToken] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<null | "webhook" | "sms">(null);

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

  const smsComplete = sid.trim() && token.trim() && from.trim() && to.trim();

  return (
    <div className="screen">
      <TopBar title="Accountability" onBack={onBack} />

      <p className="t-body text-text-2">
        Give a trusted person a heads-up if you ever weaken your protection. They
        get a short note the moment protection is turned off or an uninstall
        starts — nothing about what you browse, ever.
      </p>

      <div className="mt-6">
        <Group>
          <Row>
            <span className="t-row-title">Partner</span>
            <span className="row-trailing t-subtitle">{anyOn ? "Connected" : "Not set"}</span>
          </Row>
        </Group>
      </div>

      {/* Channel 1: text message via the user's own Twilio account. */}
      <div className="mt-8">
        <GroupLabel>Text message</GroupLabel>
        {smsOn ? (
          <>
            <Group>
              <Row>
                <span className="t-row-title">SMS</span>
                <span className="row-trailing t-subtitle">Connected</span>
              </Row>
            </Group>
            <div className="mt-3 flex justify-center">
              <Button variant="destructive" onClick={() => setConfirmRemove("sms")} disabled={busy || locked}>
                Remove SMS
              </Button>
            </div>
          </>
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
                run(
                  { cmd: "set_accountability_sms", sid: sid.trim(), token: token.trim(), from: from.trim(), to: to.trim(), password: "" },
                  "SMS connected. Sent a hello text — ask them to confirm.",
                ).then(() => {
                  setSid(""); setToken(""); setFrom(""); setTo("");
                })
              }
            >
              Connect SMS
            </Button>
          </>
        )}
        <GroupFootnote>
          Uses your own Twilio account (about 1¢ per text). Your auth token is
          stored only on this PC, in Sanctum's protected local store.
        </GroupFootnote>
      </div>

      {/* Channel 2: chat-app / push webhook (Discord, Slack, ntfy…). */}
      <div className="mt-8">
        <GroupLabel>Chat app or push (webhook)</GroupLabel>
        {webhookOn ? (
          <>
            <Group>
              <Row>
                <span className="t-row-title">Webhook</span>
                <span className="row-trailing t-subtitle">Connected</span>
              </Row>
            </Group>
            <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="New webhook URL (to change)" spellCheck={false} autoCapitalize="none" className="field mt-3" />
            <div className="mt-3 flex flex-col gap-3">
              <Button
                disabled={busy || !url.trim() || locked}
                onClick={() =>
                  run({ cmd: "set_accountability", webhook: url.trim(), password }, "Webhook updated. Your previous partner was told it changed.").then(() => {
                    setUrl(""); setPassword("");
                  })
                }
              >
                Save new webhook
              </Button>
              <div className="flex justify-center">
                <Button variant="destructive" onClick={() => setConfirmRemove("webhook")} disabled={busy || locked}>
                  Remove webhook
                </Button>
              </div>
            </div>
          </>
        ) : (
          <>
            <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="Webhook URL (Discord, Slack, ntfy…)" spellCheck={false} autoCapitalize="none" className="field" />
            <Button
              className="mt-3"
              disabled={busy || !url.trim()}
              onClick={() =>
                run({ cmd: "set_accountability", webhook: url.trim(), password: "" }, "Connected. Sent them a hello — ask them to confirm.").then(() => setUrl(""))
              }
            >
              Connect webhook
            </Button>
          </>
        )}
        <GroupFootnote>
          Make an Incoming Webhook in Discord/Slack, or use an ntfy.sh topic, and
          paste its URL. The signal goes straight to them; Sanctum keeps no copy.
        </GroupFootnote>
      </div>

      {anyOn && (
        <div className="mt-8">
          <Button variant="secondary" onClick={() => run({ cmd: "test_accountability" }, "Test sent — ask them to confirm it arrived.")} disabled={busy}>
            Send a test to your partner
          </Button>
          {hasPassword ? (
            <GroupFootnote>Changing or removing a channel needs your password and alerts your partner first.</GroupFootnote>
          ) : (
            <GroupFootnote>Set a password on the Protection screen so a channel can't be removed on a whim.</GroupFootnote>
          )}
        </div>
      )}

      {/* Password needed for change/remove when one is set. */}
      {anyOn && hasPassword && (
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Your password (to change or remove)"
          className="field mt-4"
        />
      )}

      {note && <p className="t-caption mt-6 text-center">{note}</p>}

      <ConfirmModal
        open={confirmRemove !== null}
        title={confirmRemove === "sms" ? "Remove SMS partner?" : "Remove webhook partner?"}
        body="Your partner will be told this accountability channel was removed, and it will stop sending. You can set one up again anytime."
        confirmLabel="Remove"
        onConfirm={() => {
          const which = confirmRemove;
          setConfirmRemove(null);
          if (which === "sms") {
            run({ cmd: "set_accountability_sms", sid: "", token: "", from: "", to: "", password }, "SMS removed. Your partner was texted that it was removed.").then(() => setPassword(""));
          } else {
            run({ cmd: "set_accountability", webhook: "", password }, "Webhook removed. They were told it was removed.").then(() => setPassword(""));
          }
        }}
        onCancel={() => setConfirmRemove(null)}
      />
    </div>
  );
}

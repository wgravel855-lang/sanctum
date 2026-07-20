import { useCallback, useEffect, useState } from "react";
import type { Status } from "../lib/types";
import { listCustomBlocks, listKeywords, sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { Group, GroupFootnote, GroupLabel, Row } from "../components/List";

export default function BlockList({
  status,
  onBack,
  refresh,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const locked = !!status?.locked;
  const hasPassword = !!status?.has_password;
  const approvalOn = !!status?.require_partner_approval;
  const pending = status?.pending_unblock ?? null;

  const keywordsOn = !!status?.block_keywords;

  const [custom, setCustom] = useState<string[]>([]);
  const [keywords, setKeywords] = useState<string[]>([]);
  const [keyword, setKeyword] = useState("");
  const [domain, setDomain] = useState("");
  const [unblockDomain, setUnblockDomain] = useState("");
  const [code, setCode] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);
  const [password, setPassword] = useState("");

  const loadCustom = useCallback(async () => {
    setCustom(await listCustomBlocks());
    setKeywords(await listKeywords());
  }, []);

  useEffect(() => {
    loadCustom();
  }, [loadCustom]);

  const addKeyword = async () => {
    const w = keyword.trim().toLowerCase();
    if (!w) return;
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "add_keyword", word: w });
    setNote(r.resp === "ok" ? `Added "${w}"` : `Couldn't add "${w}"`);
    setKeyword("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const removeKeyword = async (w: string) => {
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "remove_keyword", word: w, password });
    if (r.resp === "ok") setNote(`Removed "${w}"`);
    else if (r.resp === "denied") setNote(r.body.reason);
    setPassword("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const add = async () => {
    const d = domain.trim().toLowerCase();
    if (!d) return;
    setBusy(true);
    const r = await sendCommand({ cmd: "add_block", domain: d });
    setNote(r.resp === "ok" ? `Added ${d}` : `Couldn't add ${d}`);
    setDomain("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const doRemove = async (d: string, pw: string) => {
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "remove_block", domain: d, password: pw });
    if (r.resp === "ok") setNote(`Removed ${d}`);
    else if (r.resp === "denied") setNote(r.body.reason);
    else setNote(`Couldn't remove ${d}`);
    setPendingRemove(null);
    setPassword("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  // Partner-approval flow: instead of a self-service remove, ask the partner.
  const requestUnblock = async (d: string) => {
    const dom = d.trim().toLowerCase();
    if (!dom) return;
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "request_unblock", domain: dom });
    if (r.resp === "ok") setNote(`Asked your partner about ${dom}. When they approve, they'll read you a code.`);
    else if (r.resp === "denied") setNote(r.body.reason);
    setUnblockDomain("");
    await refresh();
    setBusy(false);
  };

  const approve = async () => {
    if (!code.trim()) return;
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "approve_unblock", code: code.trim() });
    if (r.resp === "ok") setNote(`Unblocked ${pending ?? "the site"}. Thank your partner.`);
    else if (r.resp === "denied") setNote(r.body.reason);
    setCode("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const onRemoveClick = (d: string) => {
    setNote(null);
    if (approvalOn) requestUnblock(d);
    else if (hasPassword) setPendingRemove(d);
    else doRemove(d, "");
  };

  return (
    <div className="screen">
      <TopBar title="Block List" onBack={onBack} />

      <p className="t-body text-text-2">
        Sanctum already blocks tens of thousands of adult sites out of the box.
        Add anything else you want kept out here.
      </p>

      {/* A pending partner-approval request, if one is in flight. */}
      {approvalOn && pending && (
        <div className="mt-8 rounded-[16px] border border-hairline bg-surface-1 p-5">
          <p className="t-row-title">Waiting for your partner's code</p>
          <p className="t-caption mt-1">
            To unblock <span className="text-text-1">{pending}</span>, ask your partner for the
            one-time code Sanctum sent them, then enter it below.
          </p>
          <input
            value={code}
            onChange={(e) => setCode(e.target.value.toUpperCase())}
            onKeyDown={(e) => e.key === "Enter" && approve()}
            placeholder="Partner's code"
            autoCapitalize="characters"
            spellCheck={false}
            className="field mt-4 text-center tracking-[0.3em]"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => requestUnblock(pending)}
              disabled={busy}
              className="pressable text-[15px] text-text-2 disabled:opacity-50"
            >
              Resend code
            </button>
            <Button onClick={approve} disabled={busy || !code.trim()}>
              Unblock
            </Button>
          </div>
        </div>
      )}

      {/* User-added sites (removable). The built-in baseline isn't user-managed
          and isn't listed or counted here. */}
      <div className="mt-8">
        <GroupLabel>Your added sites</GroupLabel>
        {custom.length === 0 ? (
          <p className="t-caption px-4">You haven't added any sites yet.</p>
        ) : (
          <Group>
            {custom.map((d) => (
              <Row key={d}>
                <span className="t-row-title truncate">{d}</span>
                <span className="row-trailing">
                  {locked ? (
                    <span className="t-caption">Locked</span>
                  ) : (
                    <button
                      onClick={() => onRemoveClick(d)}
                      disabled={busy || (approvalOn && !!pending)}
                      className="pressable text-[15px] text-destructive disabled:opacity-50"
                    >
                      {approvalOn ? "Request unblock" : "Remove"}
                    </button>
                  )}
                </span>
              </Row>
            ))}
          </Group>
        )}

        {pendingRemove && (
          <div className="mt-3">
            <p className="t-caption mb-2 px-1">
              Enter your password to remove <span className="text-text-1">{pendingRemove}</span>.
            </p>
            <input
              type="password"
              autoFocus
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doRemove(pendingRemove, password)}
              placeholder="Password"
              className="field"
            />
            <div className="mt-3 flex items-center justify-between">
              <button
                onClick={() => {
                  setPendingRemove(null);
                  setPassword("");
                }}
                className="pressable text-[15px] text-text-2"
              >
                Cancel
              </button>
              <Button variant="destructive" onClick={() => doRemove(pendingRemove, password)}>
                Remove site
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Ask a partner to unblock a site that isn't your own (an allowlist
          exception on the built-in list). Only when approval is required. */}
      {approvalOn && !pending && !locked && (
        <div className="mt-8">
          <GroupLabel>Ask to unblock a site</GroupLabel>
          <input
            value={unblockDomain}
            onChange={(e) => setUnblockDomain(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && requestUnblock(unblockDomain)}
            placeholder="example.com"
            spellCheck={false}
            autoCapitalize="none"
            className="field"
          />
          <Button className="mt-3" onClick={() => requestUnblock(unblockDomain)} disabled={busy}>
            Request approval
          </Button>
          <GroupFootnote>Sanctum sends your partner a one-time code for this exact site. Only they can approve it.</GroupFootnote>
        </div>
      )}

      {/* Keyword rules. Domain-name matching only — never page content. */}
      <div className="mt-8">
        <GroupLabel>Your keywords</GroupLabel>
        {keywords.length === 0 ? (
          <p className="t-caption px-4">
            No keywords of your own yet. Sanctum already uses a built-in set.
          </p>
        ) : (
          <Group>
            {keywords.map((w) => (
              <Row key={w}>
                <span className="t-row-title truncate">{w}</span>
                <span className="row-trailing">
                  {locked ? (
                    <span className="t-caption">Locked</span>
                  ) : (
                    <button
                      onClick={() => removeKeyword(w)}
                      disabled={busy}
                      className="pressable text-[15px] text-destructive disabled:opacity-50"
                    >
                      Remove
                    </button>
                  )}
                </span>
              </Row>
            ))}
          </Group>
        )}
        <input
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addKeyword()}
          placeholder="word to block"
          spellCheck={false}
          autoCapitalize="none"
          className="field mt-3"
        />
        <Button className="mt-3" onClick={addKeyword} disabled={busy}>
          Add keyword
        </Button>
        <GroupFootnote>
          {keywordsOn
            ? "Matched against a site's web address only, never the page itself. Words shorter than five letters must match whole, so essex.ac.uk keeps working."
            : "Turn on Block by keyword in Protection for these to take effect. They match a site's web address only, never the page itself."}
        </GroupFootnote>
      </div>

      <div className="mt-8">
        <GroupLabel>Add a site</GroupLabel>
        <input
          value={domain}
          onChange={(e) => setDomain(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="example.com"
          spellCheck={false}
          autoCapitalize="none"
          className="field"
        />
        <Button className="mt-3" onClick={add} disabled={busy}>
          Add site
        </Button>
        {note && <p className="t-caption mt-2 text-center">{note}</p>}
      </div>

      <GroupFootnote>
        {approvalOn
          ? "Unblocking needs your partner's approval: they read you a one-time code for the site you asked about. Adding sites is always allowed."
          : "You can always add sites. During a locked session the list can only grow, so removing a site is disabled until the lock ends. That friction is the point."}
      </GroupFootnote>
    </div>
  );
}

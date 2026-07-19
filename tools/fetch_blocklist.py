#!/usr/bin/env python3
"""Fetch and compile Sanctum's bulk blocklists.

Two categories, each compiled from a well-maintained upstream, validated with
the same rules as sanctum-core's domain::is_valid_host, and pruned so no entry
is a child of another listed entry (Sanctum matches by suffix, so a parent
already covers every subdomain):

  adult    -> blocklist/adult-domains-full.txt
              StevenBlack porn-only hosts (MIT; aggregates Sinfonietta).
  bypass   -> blocklist/bypass-domains.txt
              hagezi DoH/VPN/Proxy/Tor bypass list (GPL-3.0).

Run from the repo root:  py tools/fetch_blocklist.py
"""

import datetime
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LABEL = re.compile(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$")


def is_valid_host(host: str) -> bool:
    """Mirror sanctum-core domain::is_valid_host."""
    if not host or len(host) > 253:
        return False
    labels = host.split(".")
    if len(labels) < 2:
        return False
    if not all(LABEL.match(l) for l in labels):
        return False
    if labels[-1].isdigit():
        return False
    return True


def parse_hosts_or_domains(text: str):
    """Yield candidate hosts from either hosts-format or plain-domain lines."""
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.startswith("!"):
            continue
        parts = line.split()
        if len(parts) >= 2 and parts[0] in ("0.0.0.0", "127.0.0.1"):
            host = parts[1]  # hosts format
        elif len(parts) == 1:
            host = parts[0]  # plain domain
        else:
            continue
        host = host.lower().rstrip(".")
        if host in ("0.0.0.0", "localhost", "localhost.localdomain", "broadcasthost"):
            continue
        yield host


def prune_children(domains: set) -> set:
    """Drop entries whose parent is also present (suffix matching covers them)."""
    def has_listed_parent(host: str) -> bool:
        idx = host.find(".")
        while idx != -1:
            if host[idx + 1 :] in domains:
                return True
            idx = host.find(".", idx + 1)
        return False

    return {d for d in domains if not has_listed_parent(d)}


def compile_list(name: str, url: str, out_rel: str, credit: str) -> int:
    print(f"[{name}] downloading {url} ...")
    with urllib.request.urlopen(url, timeout=180) as resp:
        text = resp.read().decode("utf-8", errors="replace")

    domains, skipped = set(), 0
    for host in parse_hosts_or_domains(text):
        if is_valid_host(host):
            domains.add(host)
        else:
            skipped += 1

    raw = len(domains)
    pruned = prune_children(domains)
    out_path = ROOT / out_rel
    today = datetime.date.today().isoformat()
    header = (
        f"# Sanctum {name} blocklist (compiled)\n"
        f"# Source: {url}\n"
        f"# {credit}. See THIRD_PARTY.md.\n"
        f"# Compiled: {today} | entries: {len(pruned)} "
        f"(from {raw} valid; {skipped} invalid skipped)\n"
        f"# Matching is by suffix; entries whose parent is also listed are pruned.\n"
    )
    out_path.write_text(header + "\n".join(sorted(pruned)) + "\n", encoding="utf-8", newline="\n")
    print(f"[{name}] valid={raw} pruned={len(pruned)} skipped={skipped} "
          f"-> {out_rel} ({out_path.stat().st_size / 1e6:.1f} MB)")
    return len(pruned)


def main() -> int:
    compile_list(
        "adult",
        "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts",
        "blocklist/adult-domains-full.txt",
        "Upstream: StevenBlack/hosts porn extension (MIT), aggregating Sinfonietta/hostfiles",
    )
    compile_list(
        "bypass",
        "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/doh-vpn-proxy-bypass-onlydomains.txt",
        "blocklist/bypass-domains.txt",
        "Upstream: hagezi/dns-blocklists DoH/VPN/Proxy/Tor bypass (GPL-3.0)",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

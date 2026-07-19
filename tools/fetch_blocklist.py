#!/usr/bin/env python3
"""Fetch and compile the bulk adult-domain blocklist.

Downloads the MIT-licensed StevenBlack porn-only hosts compilation (which
aggregates Sinfonietta's hostfiles), extracts the domains, validates them with
the same rules as sanctum-core's domain::is_valid_host, prunes entries whose
parent domain is also listed (Sanctum matches by suffix, so a parent entry
already covers every subdomain), and writes a sorted, deduplicated list to
blocklist/adult-domains-full.txt for compile-time embedding.

Run from the repo root:  py tools/fetch_blocklist.py
"""

import datetime
import re
import sys
import urllib.request
from pathlib import Path

SOURCE_URL = "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts"
OUT_PATH = Path(__file__).resolve().parent.parent / "blocklist" / "adult-domains-full.txt"

LABEL = re.compile(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$")


def is_valid_host(host: str) -> bool:
    """Mirror sanctum-core domain::is_valid_host: 2+ labels, each 1..=63
    alnum/hyphen chars with no edge hyphen, non-numeric TLD, total <= 253."""
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


def main() -> int:
    print(f"downloading {SOURCE_URL} ...")
    with urllib.request.urlopen(SOURCE_URL, timeout=120) as resp:
        text = resp.read().decode("utf-8", errors="replace")

    domains = set()
    skipped = 0
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        # hosts format: "0.0.0.0 domain"
        if len(parts) < 2 or parts[0] not in ("0.0.0.0", "127.0.0.1"):
            continue
        host = parts[1].lower().rstrip(".")
        if host in ("0.0.0.0", "localhost", "localhost.localdomain", "broadcasthost"):
            continue
        if is_valid_host(host):
            domains.add(host)
        else:
            skipped += 1

    raw_count = len(domains)

    # Prune entries whose parent is also present: suffix matching means the
    # parent already blocks them. Never collapse further than what's listed.
    def has_listed_parent(host: str) -> bool:
        idx = host.find(".")
        while idx != -1:
            parent = host[idx + 1 :]
            if parent in domains:
                return True
            idx = host.find(".", idx + 1)
        return False

    pruned = {d for d in domains if not has_listed_parent(d)}

    today = datetime.date.today().isoformat()
    header = (
        "# Sanctum bulk adult-domain blocklist (compiled)\n"
        f"# Source: {SOURCE_URL}\n"
        "# Upstream: StevenBlack/hosts porn extension (MIT), aggregating\n"
        "#           Sinfonietta/hostfiles. See THIRD_PARTY.md.\n"
        f"# Compiled: {today} | entries: {len(pruned)} "
        f"(from {raw_count} valid hosts; {skipped} invalid skipped)\n"
        "# Matching is by suffix: an entry blocks the domain and all subdomains,\n"
        "# so entries whose parent is also listed have been pruned.\n"
    )
    body = "\n".join(sorted(pruned))
    OUT_PATH.write_text(header + body + "\n", encoding="utf-8", newline="\n")

    print(f"valid hosts:      {raw_count}")
    print(f"after pruning:    {len(pruned)}")
    print(f"invalid skipped:  {skipped}")
    print(f"wrote {OUT_PATH} ({OUT_PATH.stat().st_size / 1e6:.1f} MB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

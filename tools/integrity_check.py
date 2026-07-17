#!/usr/bin/env python3
"""Sanctum integrity check.

Validates, with a nonzero exit code on any failure:
  1. Every blocklist file parses cleanly.
  2. No duplicate domains within a list.
  3. Every domain is a plausible hostname.
  4. The safesearch map is well-formed (two columns).
  5. The hosts-section markers are balanced (0 or exactly 1 well-formed
     section) in a given hosts file (defaults to the live Windows hosts
     file if present).

Usage:
    python tools/integrity_check.py [--hosts PATH]

No third-party dependencies; runs on stock Python 3.8+.
"""
from __future__ import annotations

import argparse
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BLOCKLIST_DIR = os.path.join(REPO, "blocklist")

HOSTS_START = "# >>> SANCTUM START"
HOSTS_END = "# <<< SANCTUM END"

# Loose but real hostname check: 2+ labels, each 1-63 chars, no leading/
# trailing hyphen, TLD not all-numeric.
LABEL = re.compile(r"^(?!-)[a-z0-9-]{1,63}(?<!-)$")


def is_valid_host(host: str) -> bool:
    if not host or len(host) > 253:
        return False
    labels = host.split(".")
    if len(labels) < 2:
        return False
    if all(not any(c.isalpha() for c in l) for l in labels[-1:]):
        # TLD is all digits/symbols -> reject (bare IP, etc.)
        if labels[-1].isdigit():
            return False
    for label in labels:
        if not LABEL.match(label):
            return False
    if labels[-1].isdigit():
        return False
    return True


def strip_comment(line: str) -> str:
    idx = line.find("#")
    return (line[:idx] if idx >= 0 else line).strip()


def check_domain_list(path: str, errors: list[str]) -> int:
    with open(path, "r", encoding="utf-8") as fh:
        lines = fh.readlines()
    seen: dict[str, int] = {}
    count = 0
    for n, raw in enumerate(lines, 1):
        domain = strip_comment(raw).lower()
        if not domain:
            continue
        count += 1
        if not is_valid_host(domain):
            errors.append(f"{os.path.basename(path)}:{n}: invalid domain '{domain}'")
        if domain in seen:
            errors.append(
                f"{os.path.basename(path)}:{n}: duplicate domain '{domain}' "
                f"(first seen line {seen[domain]})"
            )
        else:
            seen[domain] = n
    return count


def check_safesearch(path: str, errors: list[str]) -> int:
    with open(path, "r", encoding="utf-8") as fh:
        lines = fh.readlines()
    seen: dict[str, int] = {}
    count = 0
    for n, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2:
            errors.append(f"safesearch.map:{n}: expected '<host> <target>', got '{line}'")
            continue
        host, target = parts[0].lower(), parts[1].lower()
        count += 1
        for label, value in (("host", host), ("target", target)):
            if not is_valid_host(value):
                errors.append(f"safesearch.map:{n}: invalid {label} '{value}'")
        if host in seen:
            errors.append(
                f"safesearch.map:{n}: duplicate host '{host}' (first line {seen[host]})"
            )
        else:
            seen[host] = n
    return count


def check_hosts_markers(path: str, errors: list[str]) -> None:
    if not os.path.exists(path):
        print(f"  hosts file not found ({path}) — skipping marker check")
        return
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as fh:
            content = fh.read()
    except PermissionError:
        print(f"  hosts file not readable ({path}) — skipping (run elevated to check)")
        return
    starts = sum(1 for l in content.splitlines() if l.strip() == HOSTS_START)
    ends = sum(1 for l in content.splitlines() if l.strip() == HOSTS_END)
    if (starts, ends) not in ((0, 0), (1, 1)):
        errors.append(
            f"hosts markers unbalanced: {starts} START / {ends} END (expected 0/0 or 1/1)"
        )
    elif (starts, ends) == (1, 1):
        s = next(i for i, l in enumerate(content.splitlines()) if l.strip() == HOSTS_START)
        e = next(i for i, l in enumerate(content.splitlines()) if l.strip() == HOSTS_END)
        if s >= e:
            errors.append("hosts END marker appears before START marker")
    print(f"  hosts markers: {starts} START / {ends} END — ok")


def main() -> int:
    ap = argparse.ArgumentParser(description="Sanctum integrity check")
    ap.add_argument(
        "--hosts",
        default=os.path.join(
            os.environ.get("SystemRoot", r"C:\Windows"),
            "System32",
            "drivers",
            "etc",
            "hosts",
        ),
    )
    args = ap.parse_args()

    errors: list[str] = []

    print("Sanctum integrity check")
    print("-----------------------")
    for name in ("adult-domains.txt", "doh-endpoints.txt"):
        path = os.path.join(BLOCKLIST_DIR, name)
        if not os.path.exists(path):
            errors.append(f"missing blocklist: {name}")
            continue
        n = check_domain_list(path, errors)
        print(f"  {name}: {n} domains parsed")

    ss = os.path.join(BLOCKLIST_DIR, "safesearch.map")
    if os.path.exists(ss):
        n = check_safesearch(ss, errors)
        print(f"  safesearch.map: {n} mappings parsed")
    else:
        errors.append("missing safesearch.map")

    check_hosts_markers(args.hosts, errors)

    print("-----------------------")
    if errors:
        print(f"FAIL — {len(errors)} problem(s):")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("PASS — all checks clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())

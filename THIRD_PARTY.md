# Third-party data

## Bulk adult-domain blocklist

`blocklist/adult-domains-full.txt` is compiled (by `tools/fetch_blocklist.py`)
from the **StevenBlack/hosts** "porn only" extension:

- Source: https://github.com/StevenBlack/hosts (alternates/porn-only)
- License: MIT
- Which aggregates: https://github.com/Sinfonietta/hostfiles (MIT)

The compiled file keeps only the domain column, validates each host, and prunes
entries whose parent domain is also listed (Sanctum blocks by suffix, so a
parent entry already covers every subdomain). To refresh it:

```powershell
py tools/fetch_blocklist.py
```

## Bypass-tool blocklist

`blocklist/bypass-domains.txt` is compiled (by the same script) from the
**hagezi/dns-blocklists** DoH/VPN/Proxy/Tor bypass list:

- Source: https://github.com/hagezi/dns-blocklists (wildcard/doh-vpn-proxy-bypass-onlydomains.txt)
- License: GPL-3.0 (the same license Sanctum ships under)

It blocks the DNS-over-HTTPS resolvers, VPN/proxy services, and Tor entry
points commonly used to route around a DNS content filter. Same validation and
parent-pruning as the adult list.

Thank you to the maintainers of these lists.

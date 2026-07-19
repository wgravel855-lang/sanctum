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

Thank you to the maintainers of these lists.

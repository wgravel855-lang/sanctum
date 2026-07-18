# Code signing

The Windows installer is unsigned by default, so SmartScreen shows an "Unknown
Publisher" / "not commonly downloaded" warning. This document sets up **free**
code signing for Sanctum through the **SignPath Foundation** open-source program,
driven by the `.github/workflows/release.yml` GitHub Actions workflow.

The signature removes the "Unknown Publisher" warning and builds SmartScreen
reputation over time. Note the trade-off: because the certificate belongs to the
SignPath Foundation (their HSM holds the key — you never touch it), the publisher
shown is **"SignPath Foundation"**, not "Sanctum". That is normal and expected
for the free OSS program.

## One-time setup

### 1. Apply to the SignPath Foundation (free, open source)

Apply at <https://signpath.org/apply>. Sanctum qualifies: it is a public,
GPLv3-licensed project, already released, with its functionality described on the
landing page and in the README.

Their requirements to have ready:

- Public source repository: `https://github.com/wgravel855-lang/sanctum`
- OSI-approved license: GPLv3 (in `LICENSE`)
- A description of what the app does and where it is downloaded (the landing page
  and the GitHub Releases page)

Approval typically takes a few days to a few weeks.

### 2. Configure the SignPath project

Once approved, in the SignPath web console create (or confirm) for the Sanctum
organization:

- A **project** with slug `sanctum`
- An **artifact configuration** for the NSIS installer (a single Authenticode
  `.exe`)
- A **signing policy** with slug `release-signing` bound to the SignPath
  Foundation certificate (there is usually also a `test-signing` policy that uses
  a self-issued test certificate — use that first to validate the pipeline)

If you name them differently, update `project-slug` / `signing-policy-slug` in
`.github/workflows/release.yml` to match.

### 3. Add the GitHub credentials

In the GitHub repo, under **Settings → Secrets and variables → Actions**:

- **Secret** `SIGNPATH_API_TOKEN` — the SignPath CI user's API token.
- **Variable** `SIGNPATH_ORGANIZATION_ID` — your SignPath organization ID (a GUID
  shown in the SignPath console).

## Cutting a signed release

Complete the one-time setup above first — until the SignPath credentials exist,
the workflow builds fine but fails at the signing step (that is expected and
harmless).

The workflow runs when you push a version tag:

```bash
git tag v0.1.9
git push origin v0.1.9
```

It will, on a Windows runner:

1. Build the three service binaries and the NSIS installer (`tools/build-installer.ps1`).
2. Rename it to `Sanctum-Windows-Setup.exe` and upload it as an artifact.
3. Submit that artifact to SignPath and wait for the signed result.
4. Attach the **signed** `Sanctum-Windows-Setup.exe` to the GitHub Release for the tag.

Because the landing page's download button points at
`/releases/latest/download/Sanctum-Windows-Setup.exe`, users automatically get
the signed installer once a signed release is the latest.

Do **not** also attach a locally built (unsigned) `Sanctum-Windows-Setup.exe` to
the same release — let the workflow provide the signed one.

## While the application is pending

Two things help immediately and cost nothing:

- **winget** — publishing to the Windows Package Manager lets users
  `winget install Sanctum`, which bypasses the browser SmartScreen download
  prompt entirely.
- Reputation builds on its own as more people download and run each release.

## Links

- SignPath Foundation: <https://signpath.org/>
- Open-source program: <https://signpath.io/solutions/open-source-community>
- GitHub Actions integration docs: <https://docs.signpath.io/trusted-build-systems/github>

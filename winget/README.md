# winget manifests

Manifests for publishing Sanctum to the **Windows Package Manager** so users can
install with no browser SmartScreen prompt:

```powershell
winget install Sanctum.Sanctum
```

The `InstallerUrl` points at the exact versioned release asset (no redirect) and
is pinned to that file's SHA-256, which winget verifies before running it.

## Submitting to winget

The manifests here mirror the layout of the community repo,
[`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs). To publish,
open a pull request adding this version's folder to that repo. The easiest way is
[`wingetcreate`](https://github.com/microsoft/winget-create):

```powershell
winget install Microsoft.WingetCreate
# Validate, then open a PR against microsoft/winget-pkgs (prompts for a GitHub token):
wingetcreate submit --token <your-github-token> winget/manifests/s/Sanctum/Sanctum/0.1.8
```

Or fork `microsoft/winget-pkgs`, copy `manifests/s/Sanctum/Sanctum/0.1.8/` into it,
and open the PR by hand. Microsoft's automated checks + a reviewer will merge it,
usually within a day or two.

Validate locally first (already passing):

```powershell
winget validate --manifest winget/manifests/s/Sanctum/Sanctum/0.1.8
```

## For each new release

Create a new versioned folder (bump `PackageVersion`, the `InstallerUrl`, and the
`InstallerSha256`) and submit it. `wingetcreate` can automate this:

```powershell
wingetcreate update Sanctum.Sanctum --version 0.1.9 --urls https://github.com/wgravel855-lang/sanctum/releases/download/v0.1.9/Sanctum-Windows-Setup.exe --submit --token <your-github-token>
```

Compute a hash manually with: `Get-FileHash <file> -Algorithm SHA256`.

Note: `PackageIdentifier` is `Sanctum.Sanctum`; change it here and in all three
YAML files if you'd prefer a different publisher segment.

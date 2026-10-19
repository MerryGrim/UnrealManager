# Verification on the restored computer

Install the source project's locked dependencies with `npm ci` in `../source`.
Use Node.js 22.12 or later and an existing Playwright package. If Playwright is
outside the project, set `UM_PLAYWRIGHT_MODULE` to its package directory. The UI
tests launch the locally installed Microsoft Edge in headless mode with GPU
acceleration and GPU compositing disabled.

```powershell
$env:UM_PLAYWRIGHT_MODULE = 'D:\Tools\playwright\node_modules\playwright'
node .\verification\run-beta6.cjs
```

The beta.6 selection checks all eight non-Russian locale catalogues, preference
migration and persistence, library filters, Vault pause/resume and installation
intent for assets/projects/plugins, saved engine task resume, confirmed removal
from the resume list, and transfer controls at 1280/600 px.

To rerun one selected test, pass its file name to the runner, for example
`node .\verification\run-beta6.cjs test-preferences.cjs`. Multiple selected file
names are supported. Each run writes logs, screenshots, and a JSON report into a
new timestamped folder under `verification/current`. Set
`UM_VERIFICATION_OUTPUT_DIRECTORY` to choose a different output directory.

Individual `test-*.cjs` scripts use the shared `test-env.cjs` helper and default
to writing screenshots into `verification/current`. Their source root is
resolved relative to this directory; the `E:/Fixture` paths are simulated input
values and do not require an E: drive. Existing logs and screenshots beside
these scripts are historical evidence from the original computer and remain
untouched.

Older tests are kept for reference. Some assert UI behavior that changed before
beta.6, so they are not all included in the current regression runner. These UI
checks mock native Tauri commands; actual filesystem operations and Windows
application behavior require the Rust tests and native smoke checks as well.

## Native idle runtime measurement

After building the Windows executable, run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\verification\measure-runtime.ps1 -Executable .\source\src-tauri\target\release\unreal-manager.exe -Seconds 20
```

The script refuses to start while Unreal Manager is already running. It copies
only the executable into a new `current/runtime-.../isolated` directory, enables
portable storage beside that copy, and sets its close behavior to exit. It
launches this copy with `-WindowStyle Hidden`, waits five seconds, then samples
the owned process tree's CPU, working set, private memory, GPU engine counters
and GPU memory counters. It also records the root WebView2 browser command line
and checks for both software rendering flags. Existing app data is not copied.

`-OutputDirectory` chooses the parent directory for a new unique run folder.
The script closes its own main window after sampling and force stops only its
own verified process identities if graceful shutdown fails. The resulting
`runtime-report.json` distinguishes software configuration from measured GPU
activity and records unavailable counters, native exit codes before/after
cleanup, and the paths to captured native stdout/stderr logs. Zero samples of GPU activity cannot
establish GPU power state or behavior during login, tray use or active transfers.

## GitHub Pages website regression

Run the static website checks without starting Unreal Manager:

```powershell
$env:UM_PLAYWRIGHT_MODULE = 'D:\Tools\playwright\node_modules\playwright'
node .\verification\test-site.cjs
```

This script serves `docs` through its own temporary HTTP server on
`127.0.0.1` under `/UnrealManager/`, then stops that server and its headless
Edge browser. It checks all nine languages in light/dark themes at 1280 px and
390 px, translated content/metadata/controls, image loading, keyboard navigation,
system/light/dark icon buttons with localized accessible names and pressed
states, mobile touch targets, system theme changes, preference persistence,
query links, denied browser storage, horizontal overflow, console errors and
unexpected network requests. Installer, portable ZIP and checksum links must
match the exact versioned GitHub Release asset URLs.
Logs, a JSON report and representative screenshots are saved into a new
`verification/current/site-...` directory. GPU flags configure the test browser;
these checks do not measure hardware GPU activity.

## Public demonstration screenshots

`capture-public-screenshots.cjs` renders the actual React application with
fabricated account, assets, projects, paths and transfer data. It does not use a
native app session or contact Epic/Fab. It additionally requires an existing
`sharp` package for lossless WebP conversion; neither screenshot dependency is
part of the app runtime.

```powershell
$env:UM_PLAYWRIGHT_MODULE = 'D:\Tools\playwright\node_modules\playwright'
$env:UM_SHARP_MODULE = 'D:\Tools\sharp\node_modules\sharp'
node .\verification\capture-public-screenshots.cjs --preview
```

`--preview` writes only to a new `verification/current/public-screenshots-...`
directory. Omit it to also update the four public images in
`docs/assets/screenshots`. Original PNG files and a provenance/hash manifest
remain in the evidence directory. Capture after updating source artwork so the
screenshots match the current application. The application screenshots use its
actual dark appearance; the website's separate light theme does not modify the
application UI.

## Static release artifact validation

After creating a fresh release staging directory, compare its portable ZIP to
the newly built executable without launching either package:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\verification\verify-release.ps1 -ReleaseDirectory 'D:\Releases\staging' -ExpectedExecutable '.\source\src-tauri\target\release\unreal-manager.exe'
```

The helper checks the versioned ZIP/installer names, portable archive root and
approved file inventory, traversal/duplicate/symlink exclusions, marker and
legal files, AMD64 PE headers, beta version metadata, and byte-for-byte portable
EXE identity using SHA-256. It records installer PE/version/hash/signature
metadata, but does not claim to have extracted or compared its compressed
payload. It never installs or launches an executable or changes user profiles
or registry settings. Reports and an extracted EXE used only for static
inspection go into a unique `verification/current/release-validation-...`
directory. Pass `-Version` for a later release, or `-OutputDirectory` to choose
the parent evidence directory.

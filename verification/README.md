# Verification

Install the source project's locked dependencies with `npm ci` in `../source`.
Use Node.js 22.12 or later and an existing Playwright package. If Playwright is
outside the project, set `UM_PLAYWRIGHT_MODULE` to its package directory. The UI
tests launch the locally installed Microsoft Edge in headless mode with GPU
acceleration and GPU compositing disabled.

```powershell
$env:UM_PLAYWRIGHT_MODULE = (Resolve-Path '.\tools\playwright\node_modules\playwright').Path
node .\verification\run-regression.cjs
```

The current selection includes the signed-update settings UI, startup deduplication,
channels, verified download/apply states, errors and retry. These are mocked UI
checks. It also checks all eight non-Russian locale catalogues, preference
migration and persistence, library filters, Vault pause/resume and installation
intent for assets/projects/plugins, saved engine task resume, confirmed removal
from the resume list, and transfer controls at 1280/600 px. The auth-required
regression covers explicit sign-in actions, all nine languages and preservation
of mocked saved downloads; it does not verify a genuinely expired Epic session.

`test-project-discovery.cjs` mounts the actual Projects view, project-folder
dialog and project tiles with a synthetic native provider. It checks saved folder
choices, explicit refresh, single-job guards, cancellation with partial results,
authoritative returned summaries when the final channel event is late or absent,
limits/errors, preserved projects, keyboard focus and nine languages. Successful
refresh uses a dismissible five-second notification; project search and its old
help paragraphs are absent. It never scans a real folder or drive; native
traversal, preference/descriptor persistence and filesystem cancellation require
their separate isolated tests.

`test-redemption-batch.cjs` exercises the actual key-entry component and its TXT
parser using synthetic IPC. It checks the three-column queue, preview before
explicit confirmation, single-use activation intent, cancellation and unknown
outcomes, import bounds, stale callbacks, account changes and the separate
official-window fallback. Synthetic keys remain in memory and are excluded from
screenshots, call-recorder values and reports. This is not evidence that a real
key was activated or that Epic's current service accepted an end-to-end batch.

To rerun one selected test, pass its file name to the runner, for example
`node .\verification\run-regression.cjs test-preferences.cjs`. Multiple selected file
names are supported. Each run writes logs, screenshots, and a JSON report into a
new timestamped folder under `verification/current`. Set
`UM_VERIFICATION_OUTPUT_DIRECTORY` to choose a different output directory.

`run-regression.cjs` is the canonical current selection; `run-beta7.cjs` remains
a compatible alias. `--list` lists the selected tests without loading browser
dependencies. `--validate` resolves existing dependencies and records the plan
and test hashes without executing tests or opening a browser. The current
selection also includes the nine-language accessibility matrix: renderer DPR
emulation, viewport layout and keyboard checks, not native Windows DPI or
assistive-technology acceptance. Each executed test receives a fresh output
subdirectory. Older fixtures which create their own timestamped folder remain
linked through their captured log and `reportedEvidenceDirectories`.

## Canonical Windows acceptance and CI

Prepare dependencies separately with the project's `npm ci`, an installed
Microsoft Edge, MSVC, Rust and an existing Playwright package. Cargo's cache
must already contain the locked Windows dependencies (`cargo fetch --locked
--target x86_64-pc-windows-msvc` is the separate online preparation step).
Nothing in the acceptance entry point installs a dependency, downloads a
browser, creates a signing key or changes machine/user environment settings.

From the repository root, with PowerShell 7:

```powershell
pwsh -NoProfile -File .\verification\run-acceptance.ps1 `
  -PlaywrightModule '<existing-playwright-package-directory>' -ValidateOnly

pwsh -NoProfile -File .\verification\run-acceptance.ps1 `
  -PlaywrightModule '<existing-playwright-package-directory>'
```

`-ValidateOnly` checks tool/module availability and writes a plan. Its report
has `passed: null`; it does not claim acceptance. A full run calls `BUILD-ENV.ps1`
for process-local paths, then runs TypeScript without emitting, Vite assets
needed by Tauri's release tests, production `cargo check --release`, library
`cargo test --release` with one test thread, the current UI/locale selection,
and privacy self-tests plus a public-source scan. Both Cargo commands use
`--locked --offline`; no `npx` or implicit install is used. A stage failure
makes the final result fail and remains in its log. Do not run this against a
Cargo target or frontend build directory another build is using.

Every invocation creates `verification/current/acceptance-<UTC>-<uuid>` with
stage timestamps, exit codes, command arguments, source/lockfile hashes, tool
versions and a final `reports/acceptance-report.json`. After a successful native
suite, that report binds its unique Cargo-emitted library test executable by
absolute path, SHA256 and, in CI, the source commit. `-OutputDirectory`
chooses a parent **inside** `verification/current`; existing reports are never
overwritten. Fresh temporary and synthetic fixture directories are recorded
and retained for exact reviewed cleanup. The entry point does not stop an app
or silently delete a directory. Runtime/GPU measurement, static distribution
inspection, clean-Windows acceptance, live accounts, website checks and actual
update transitions remain separately authorized checks.

Signed updater subprocess fixtures are deliberately opt-in and local:

```powershell
pwsh -NoProfile -File .\verification\run-acceptance.ps1 `
  -PlaywrightModule '<existing-playwright-package-directory>' `
  -IncludeSignedUpdater -SigningKeyPath '<existing-current-user-DPAPI-key>'
```

This mode uses the test executable emitted by **that run's successful Cargo
suite**. It requires the existing matching signing key and C# fixture compiler;
it does not generate a replacement key or publish the signed synthetic files.
It may launch owned fixture subprocesses. It is refused when `CI` is set and
does not replace full installed/portable application-update acceptance. The
default run never accesses the signing key.

`.github/workflows/verify-windows.yml` runs the same default entry point on
Windows Server 2022 for pull requests, main changes, the explicit isolated
`codex/acceptance` branch and manual dispatch. Pinned Node/Rust/Playwright tools
and locked dependencies are provisioned online before offline acceptance. New
downloads, caches and outputs stay on the runner workspace volume; Edge and
MSVC come from the hosted image. Actions are SHA-pinned, repository permission
is `contents: read`, and checkout does not persist credentials.

After acceptance, a hosted-only wrapper exercises actual disk-full handling
on its own temporary VHD, using exactly that run's compiled test executable
and hash. The ignored disk-full test is never run on the developer host by the
default entry point. The primary job then builds unsigned CI distributions
once with the warm target. Separate Windows Server 2022/2025 jobs download the
same-run artifact by immutable ID and exercise installer and portable lifecycle
with synthetic profile data and actual UI Automation readiness. These are
hosted Server results, not clean Windows 10/11, standard-user or live-account
acceptance. See [distribution/HOSTED-LIFECYCLE.md](distribution/HOSTED-LIFECYCLE.md).

There are no signing secrets, release uploads or deployment steps. Reports,
logs and fabricated UI evidence expire after 14 days; the unsigned CI package
artifact expires after one day and is not a publishable application update.
The workflow must actually pass on GitHub before claiming CI acceptance; local
syntax/plan validation alone does not establish that result.

Individual `test-*.cjs` scripts use the shared `test-env.cjs` helper and default
to writing screenshots into `verification/current`. Their source root is
resolved relative to this directory. Paths beginning with `X:/Fixture` are
synthetic input values used by mocks and do not require that drive to exist.
Generated evidence goes under the ignored `verification/current` directory;
archived logs and screenshots remain untouched.

Older tests are kept for reference. Some assert UI behavior that changed before
beta.6, so they are not all included in the current regression runner. These UI
checks mock native Tauri commands; actual filesystem operations and Windows
application behavior require the Rust tests and native smoke checks as well.

`test-engine-recovery-progress.cjs` covers manual recovery's native progress
channel, per-file/read-byte metrics, current-transfer propagation, blocked
dismissal while busy, and completion/failure states. It uses synthetic paths
and mocked native commands. Native rollback tests separately verify restored
file contents and events emitted while hashing large files.

`test-app-startup-ready.cjs` mounts the actual application entry point with
mocked native commands. It covers failed preference initialization, an aborted
initial render, StrictMode deduplication, optional pending scans and a rejected
acknowledgement. These browser checks do not install an update. The native
subprocess fixtures and isolated application runs establish separate evidence.

## Native idle runtime measurement

See [release gates](RELEASE-GATES.md) for the acceptance boundary and
[the disposable Windows kit](distribution/README.md) for clean-OS cases.
No clean Windows acceptance is inferred from tests on a development host.

For an observed workload use `-Seconds 300 -PhaseFile <absolute-label-file>`.
The optional file contains one label: `idle`, `tray`, `login-open`, `transfer`,
or `unclassified`. The collector reads this label; it does not drive the UI,
authenticate or start a transfer. Its report records CPU/private-memory
summaries per observed phase. Inspect the actual workload separately.

Immediately after measurement on the same boot, run
`./verification/classify-runtime-gpu.ps1 -RuntimeReportPath <runtime-report.json>`.
This maps owned GPU counter LUIDs to DXGI adapters and distinguishes software
from hardware rows. Missing hardware rows remain unavailable telemetry.
`test-runtime-evidence.ps1` checks the collector's process-identity cleanup and
flag-observation history; it is included in the default acceptance command.
Late unreadable process metadata stays in the report and cannot inherit
evidence from a reused PID.

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
$env:UM_PLAYWRIGHT_MODULE = (Resolve-Path '.\tools\playwright\node_modules\playwright').Path
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
$env:UM_PLAYWRIGHT_MODULE = (Resolve-Path '.\tools\playwright\node_modules\playwright').Path
$env:UM_SHARP_MODULE = (Resolve-Path '.\tools\sharp\node_modules\sharp').Path
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
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\verification\verify-release.ps1 -ReleaseDirectory '.\Releases\staging' -ExpectedExecutable '.\source\src-tauri\target\release\unreal-manager.exe'
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

## Signed native updater fixtures

Build the library tests, then pass the emitted unit-test EXE to the opt-in
subprocess harness:

```powershell
cargo test --manifest-path .\source\src-tauri\Cargo.toml --locked --offline --lib
node .\verification\test-update-native.cjs --test-executable '<unit-test-exe>' --powershell '<pwsh-exe>'
```

The harness requires the release signing key already protected with the current
user's DPAPI, PowerShell 7, and an existing x64 C# compiler for tiny test-only
programs. `--private-key`, `--csharp-compiler`, `--fixture-version`, and
`--output-directory` override their defaults. It copies signing metadata into a
unique `verification/current/native-update-...` directory and signs a strictly
newer synthetic version there; it never publishes those assets or changes
production version metadata, the public key, or the private key.

The actual helper validates the production signature and payload hashes, binds
the parent PID to its creation time and executable path, waits for its exit,
applies the portable payload, launches the synthetic native-ready application,
and rolls back on failure. Installed-mode checks use a separate mock installer
process with the exact Inno command arguments and same-directory file copies.
These checks preserve fabricated account, task, project and marker files and
reject a tampered payload before asking the parent to exit. The EXE entry used
to orchestrate tests exists only under Rust `cfg(test)` and starts no WebView.
The crash probe terminates its own helper while a mock installer and its child
are running, verifies that the Windows Job Object stops both processes, and
checks that interrupted installation keeps its journal and backups for manual
recovery.

`--installer-kind inno` instead compiles an isolated Inno Setup fixture without
uninstall/registry registration. This optional case requires `--inno-compiler`
or an existing compiler in the tools directory. Local endpoint protection may
block a newly generated fixture; do not disable protection to run it. The JSON
report records which installer kind was actually tested. Mock subprocess checks
do not establish a full production Inno installation or a live GitHub update.
The fixture must keep `CreateAppDir=yes` so `{app}` follows the isolated `/DIR`
target. Its five installed file hashes are compared to the supplied portable
files; uninstall registration, shortcuts, and automatic app launch stay disabled.

For an isolated full application fixture, pass `--fixture-app`,
`--fixture-portable-directory` (the exact five files used by its installer), and
`--fixture-setup`. The supplied app must require `UM_FULL_UPDATE_FIXTURE_DATA`
as its data root; the harness sets it to that case's new `Data` directory,
disables automatic update checks there and sets close behavior to exit. This
mode runs commit and tamper-refusal cases using the app's actual native-ready
acknowledgement. It records available owned WebView2 browser software flags,
then closes only the verified fixture process with WM_CLOSE and a verified
fallback. These flags describe configuration, not measured GPU activity.
`--case` selects one named scenario for a targeted rerun.

## Public-file and binary path privacy

The read-only checker inspects current tracked and non-ignored new Git files.
It excludes local data, release staging and `verification/current` from that
source inventory. Pass an EXE and/or portable folder to inspect release bytes:

```powershell
node .\verification\check-public-privacy.cjs --self-test
$report = Join-Path '.\verification\current' ('privacy-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff') + '.json')
node .\verification\check-public-privacy.cjs --exe '.\source\src-tauri\target\release\unreal-manager.exe' --portable '.\Releases\staging\UnrealManager-Portable' --output $report
```

It searches for the workspace, configured development tool roots, user profile
and account-name aliases in UTF-8 and UTF-16LE at even and odd byte offsets,
including escaped Windows paths. Reports contain labels and counts rather than
the matched personal values; an existing output report is never overwritten.
Public URLs and declared public author attribution are counted separately from
private account paths. The check also lists any WebP EXIF/XMP/ICC chunks.
Compressed payload contents and text drawn into image pixels need separate
artifact or visual inspection.

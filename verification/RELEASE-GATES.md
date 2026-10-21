# Acceptance before 1.0

Record the exact candidate SHA-256 and source revision for each run. Native
source tests, mocked renderer tests, an actual installed lifecycle and a real
account transfer establish different scopes. Unexecuted cases remain open.

## Verified local scope — 2026-10-21

Final local source is [9c2dd9a](https://github.com/MerryGrim/UnrealManager/commit/9c2dd9addb46fed1aac99f5eb71d5bffc2745d85).
The default EXE has SHA-256
`8667f5b066feb872748b21adb1b928dcf0dd87718ef652e6ff2749ee057dca33`.
Local packaging did not launch that EXE or Setup.

- The native suite passed 238 tests, with no failures and six explicit special
  entries ignored by the ordinary runner. Subprocess entries and synthetic
  timing benchmarks are separate opt-in cases; a skipped entry is not a pass.
- The final renderer selection passed all 14 groups, including 72 combinations
  of nine languages, four renderer pixel scales and two viewport sizes. These
  pixel scales do not change the Windows display scale.
- A default production build, Setup and portable ZIP passed 57 static package
  checks. All 252 public source files and the inspected runtime/legal files had no private
  workspace, tool or user-profile path findings. No signing secret was used by
  the hosted workflow and no release or update feed was changed.
- In a separate portable profile, a human completed a fresh Epic sign-in. The
  remembered session survived replacement with queue candidate `8030e2dad503` and normal
  restarts. A real completed Fab download matched all 205 stored file hashes
  and its aggregate size. A second real download passed pause, continue,
  application exit, restart and manual resume checks. These real account checks
  preceded the API-argument change. No full Unreal Engine
  installation, key activation or new-account permissions claim follows.
- Actual native UI on `8030e2dad503` confirmed that a manually resumed download appears once in
  Current, without its stale attention row or a duplicate queue count. Stop
  restored the saved task and queue row. The display-only correction leaves
  persisted queue, ordering and revision semantics unchanged.
- A 240-second owned-process measurement on auth candidate `069c09379ba4`
  recorded transfer and paused phases. The display-only queue candidate had a
  separate native UI/software-flag check. All readable browser roots carried
  both software-rendering flags. Observed GPU rows mapped to the software
  adapter; missing hardware rows remain unavailable telemetry, not measured
  zero. Paused measurements are not an idle baseline. This is not a measurement
  of the final API-policy EXE.
- All four WebView2 constructors now receive the software arguments through
  the builder API before creation, including the hidden Epic session window.
  Locked SDK defaults and the environment fallback are retained.
  [Microsoft documents that elevated hosts ignore environment overrides but
  honor API configuration](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags).
  Constructor coverage and compilation establish configuration, not measured
  GPU utilization or a new real Epic interaction in those windows.
- A real registration-broker invocation returned the native cancellation
  result and removed its owned fixture. The human reported that no UAC prompt
  appeared. Human cancellation is therefore **not verified**; the result does
  not distinguish a timeout, policy rejection or another OS cancellation.
- Owned local test processes exited normally. The owned local test profile,
  its cached session/task files, test downloads and temporary work files were
  removed after identity and inventory checks. Reports, source and frozen
  packages were retained. The original installed profile, Vault, tasks and user
  projects were not selected. No permanent helper service or monitor was added.

## Hosted workflow scope

The [Windows workflow](../.github/workflows/verify-windows.yml) runs ordinary
source checks and a real atomic-write disk-full case on a newly created,
64 MiB NTFS VHD. It verifies old bytes and modification time, removal of the
failed temporary file, successful retry after space is freed, and exact-image
detach/backing-file cleanup. It never fills a shared or physical host volume.
This is filesystem-exhaustion evidence, not physical power-loss testing.

The same workflow builds unsigned test packages once and supplies those exact
bytes to separate Windows Server 2022 and 2025 jobs. The lifecycle must establish
actual React navigation/version readiness, software flags, normal shutdown,
Setup uninstall/reinstall and portable marker/synthetic-data preservation.
Process creation or an installed EXE hash alone does not establish UI readiness.

Earlier failed runs are retained: [first run](https://github.com/MerryGrim/UnrealManager/actions/runs/39577574722)
passed source and disk-full checks, then found a compiler-version harness error;
[second run](https://github.com/MerryGrim/UnrealManager/actions/runs/39579545861)
also built packages and installed their exact EXE, then failed startup identity
inspection before UI readiness. The [third run](https://github.com/MerryGrim/UnrealManager/actions/runs/39582931852)
passed source, disk-full and package stages, then timed out in the UIAutomation
probe on both Server images. Its Server 2025 report observed all three actual
React navigation buttons, but did not establish the displayed version. Cleanup
also inspected an uninstaller child before it finished.
The [fourth run](https://github.com/MerryGrim/UnrealManager/actions/runs/39587602906)
passed all 17 lifecycle assertions on Server 2022. Server 2025 established actual
React UI but failed aggregate software flags. Both cleaned their owned data.
The old aggregate lacks individual switch forms, so it cannot retrospectively
distinguish missing switches from a quoting-parser false negative. These failed
reports are retained without converting their failures to passes.
The close assertions in the fourth and fifth runs did not attest a native zero
exit code; their historical passes do not satisfy the newer strict close gate.

The [fifth run](https://github.com/MerryGrim/UnrealManager/actions/runs/39591194721)
observed both exact bare software flags on both Server images. Server 2022
passed all 17 assertions; Server 2025 passed first UI/flags/close/uninstall,
then exposed a late Inno worker: reinstall began before the old uninstall had
completed and created `unins001`. Its registration/profile cleanup failed and
retained the inert fixture. The old close assertion did not attest a native
zero exit code. That failed scope is not converted to a pass.

The [sixth run](https://github.com/MerryGrim/UnrealManager/actions/runs/39596501418)
waited for completed operation logs, absent old EXE/DAT, identity-bound workers
under the fresh owned temp root and a stable quiescent interval. It pins the
registered direct-child uninstaller pair and records requested normal close,
process-object exit-code fields and observed descendant absence. Runtime source remains
`9c2dd9a`; newer changes are verification-only. Source, disk-full and package
stages passed, but both lifecycle jobs failed. Server 2022 observed actual UI
and software flags; its process object returned a null exit code after the
close request, so normal closure was not accepted. Server 2025 spent almost
30 seconds starting the separate UIA process before the original 35-second
parent timeout. Neither completed uninstall/reinstall/portable checks, and both
retained their fixtures under the uncertainty guards. The corrected harness
uses a retained, identity-verified native process handle and separate bounded
startup/probe budgets. Its 45 isolated helper checks and 11 real headless
exit-code checks passed.

The [seventh run](https://github.com/MerryGrim/UnrealManager/actions/runs/39600332215)
passed source checks, the real VHD disk-full case, package creation and all
17 lifecycle assertions on **both Server 2022 and Server 2025**. Each image
established three actual navigation/version-ready launches, three readable
browser roots with both exact bare software flags, and three requested normal
closes with native exit code zero and observed descendants absent. Each image
completed four installer waits with closed success logs and stable observed
quiescence; both uninstalls confirmed the old EXE/DAT absent. The blank installed
profile and portable fixture were removed with absence confirmed. Runtime
source remains `9c2dd9a`; the tested harness revision is `4a758e7`. Exact CI
candidate hashes are recorded separately from the final local build above.
These elevated Server images already contained WebView2, so clean desktop,
standard-user/UAC and missing-runtime acceptance remain open. No release,
update feed or signing key is used by CI.

## Remaining acceptance

| Gate | Remaining scope |
| --- | --- |
| Clean desktop distribution | Clean Windows 10 and 11 consumer images, standard-user install/uninstall/reinstall, and genuinely missing-WebView2 online/offline paths. Hosted Server images include development tools and are a separate scope. |
| Account lifecycle | Fresh installed-mode and legitimately expired Epic sessions. Simulated API 401 tests do not establish a genuine expiry or new-account permission flow. |
| Windows permission interaction | Human cancellation of a visible UAC prompt; the local OS cancellation result above is insufficient. |
| Native layout | Additional actual Windows display scales and short work areas. Renderer/DPR checks do not establish those OS settings. |
| Runtime baselines | Separate idle/tray measurements. The authenticated transfer/paused run is not an idle baseline; missing hardware GPU counter rows leave hardware utilization unverified. |
| Destructive storage conditions | Physical storage faults or power loss only where a product claim requires them; the VHD and process-termination fixtures have narrower scope. |

Use the [acceptance command](README.md) after separate dependency provisioning.
Signed updater subprocess fixtures remain local opt-in and use the existing
key; CI creates no key. Keep failed reports, write reruns into fresh output
folders, and retain only redacted reports from disposable guests. No permanent
monitor, helper service or graphics effect is required by these checks.

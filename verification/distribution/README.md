# Clean Windows distribution acceptance kit

This kit is prepared, not executed on a clean OS. It runs with Windows PowerShell 5.1 already supplied by Windows 10/11. It requires no Node, Rust, compiler, Playwright, Python, guest integration tool, new service, or changed Windows feature.

Use a **disposable x64 Windows 10 or Windows 11 VM**, a fresh standard-user account and a restorable snapshot. A real clean OS image and licence are supplied by the operator. No OS image is downloaded by this kit. Hardware/Hyper-V availability on the preparation machine does not establish a configured test VM.

The script refuses all runtime actions on the preparation machine (pinned hashed MachineGuid), on unrecognized physical models, without `-DisposableVm` and a generated `-ForbiddenHostContextPath`, or where an existing Unreal Manager profile/uninstall record is found. Do not edit the context to bypass this guard. Use a new guest snapshot for every matrix row. Standard-user execution matters: do not launch PowerShell elevated for these per-user installer tests.

## Inputs and provenance

On the preparation host, run `New-DistributionKit.ps1 -OutputDirectory <fresh-private-output>`. It copies the reusable scripts and generates `forbidden-host-context.json` in that new private directory. The context contains a hashed host identity; keep it out of source control and published artifacts. Copy the generated kit and one frozen `Packages/` directory into a guest workspace on its configured work volume. `update-manifest.json`, `update-manifest.sig`, `SHA256SUMS.txt`, Setup, Portable ZIP and Portable.umupdate are required. Never include `.release-secrets`, private signing keys, real account/profile data, or authenticated download URLs.

`VerifyPackages` checks the existing pinned P-256/P1363 public-key signature, manifest target, canonical GitHub payload URLs, payload sizes, checksums, fixed six-file ZIP layout and EXE hash against the signed manifest. It does not execute or extract the EXE and is the only action allowed on the preparation host. A valid update-manifest signature is separate from Windows Authenticode publisher signing.

```powershell
$kit = 'X:\QA\kit\Invoke-DistributionCase.ps1'
$packages = 'X:\QA\Packages'
& $kit -Action VerifyPackages -PackagesDirectory $packages -OutputDirectory 'X:\QA\package-preflight-001'
```

Use your guest workspace drive instead of the synthetic `X:` examples. Every output directory must be new. Logs and receipts stay local. Temp/TMP for child installer/app actions are scoped to the case's `Temp` folder. Installed-mode data deliberately uses the guest's real `%LOCALAPPDATA%\UnrealManager\Data` so its production storage contract is exercised **only inside the disposable guest**.

## One installed case: existing WebView2, offline

On a fresh VM snapshot with WebView2 registered, disconnect the VM NIC in the hypervisor. This kit never changes guest adapters, firewall, DNS or proxy. `-OfflineConfirmed` records the operator's condition; it is not an independent packet-capture proof of no network.

```powershell
$case = 'X:\QA\win11-installed-runtime-offline-001'
$guest = @{ OutputDirectory=$case; DisposableVm=$true; ForbiddenHostContextPath='X:\QA\kit\forbidden-host-context.json' }
& $kit -Action Initialize @guest -PackagesDirectory $packages -Mode installed -ExpectedWebView present -Network offline -OfflineConfirmed
& $kit -Action Install @guest
& $kit -Action Launch @guest -ShowWindow
# Inspect actual main UI in this guest; do not call process creation a UI pass.
& $kit -Action Observe @guest -Observation MainUi -Outcome Pass -Note 'Main navigation, settings and version visible; no runtime error.'
& $kit -Action Snapshot @guest
& $kit -Action Close @guest
& $kit -Action Uninstall @guest
& $kit -Action Reinstall @guest
& $kit -Action Launch @guest -ShowWindow
& $kit -Action Observe @guest -Observation UninstallReinstall -Outcome Pass -Note 'Reinstalled UI opens; synthetic settings and sentinels retained.'
& $kit -Action Close @guest
& $kit -Action Uninstall @guest
& $kit -Action Finish @guest
```

Install uses a unique app directory, native Inno logging, standard-user silent execution and fixed product AppId. Exit code, signed EXE hash, absence of portable marker, runtime registration and preservation sentinels are checked. Uninstall checks EXE removal, uninstall registry removal and preservation of app data. Reinstall preserves the same synthetic profile. The kit never deletes a real profile or guesses ownership from a process name.

`Close` targets only recorded PID + creation FILETIME + exact EXE path, then checks the observed owned descendant identities. It never force-kills, restarts or controls unrelated processes. A timeout or remaining child fails conservatively and preserves evidence; inspect/revert the disposable VM instead of killing an installer.

## Missing WebView2 and bootstrapper cases

Create a guest image that genuinely lacks WebView2. The kit will **not uninstall WebView2** or forge its registry to simulate absence. `Initialize -ExpectedWebView missing` refuses a guest whose runtime registration is present. Detection matches Setup's four HKCU/HKLM 32/64-bit registry checks, rather than claiming the registry proves full runtime health.

For **online** installed mode, initialize with `-Network online`, run `Install`, inspect main UI, snapshot, close, uninstall. Setup embeds the Microsoft-signed **online Evergreen bootstrapper**, not a complete offline WebView2 runtime. Bootstrapper network/proxy/UAC behavior remains actual VM acceptance; the kit does not infer it from package validation.

For **offline + missing runtime**, initialize installed mode with `-ExpectedWebView missing -Network offline -OfflineConfirmed`, then `Install`. The expected result is a nonzero Setup code, no copied application EXE and runtime registration still absent. Inspect the retained setup log and visible error if running interactive testing; a generic download/network failure is not a successful offline installation. The kit records this as an **expected failure-path pass**, not a working offline app. Run `Finish`, then reset the guest.

For portable mode, initialize with `-Mode portable`, then `Launch -ShowWindow`. With runtime present, record UI, snapshot and close. With runtime absent, record the actual missing-runtime error/exit under `OfflineError`; `Launch` deliberately reports only process creation. If there is no actionable error, mark the row `Fail` and keep the observation. No fake UI readiness is inferred. Do not invoke `Install` for portable cases.

## Epic fresh and expired sessions

These are explicit **manual, online** guest checks, using a test account entered only in the official Epic window. Start from a fresh snapshot/profile. `Account/session.fixture` is an inert synthetic preservation sentinel, not a DPAPI session, credential or sign-in bypass.

1. Fresh profile: inspect signed-out state, open Epic login, sign in manually, return to app, close/relaunch, verify expected account state. Open redemption and verify the same test account; leave the key empty and never activate anything. Record `EpicFreshLogin` and `RedemptionAccount` as manual observations.
2. Expired/revoked profile: obtain a **legitimately expired or server-revoked guest test session**, not a fabricated/edited DPAPI file or replayed token. Reopen the app, verify actionable reauthentication behavior and successful manual sign-in without losing synthetic tasks/settings. Record `EpicExpiredReauth`. If no genuinely expired session is available, use `NotRun` with that limitation; this gate stays open.
3. The script never reads, copies, decrypts, exports or prints the actual session, tokens, cookies, catalogue or account name. Observation text rejects common credential markers and email-like values. Screenshots or external logs are not automatically scrubbed: inspect them separately before sharing. Only redacted summaries should leave the disposable guest.

For locale/keyboard/scale acceptance, inspect Russian/English plus all remaining app locales, long German/Japanese text, Tab/Shift+Tab/Escape, window close behavior and 100%/150%/200% display scale on appropriate guest work areas. Record `NineLocales`/`KeyboardAndScale` with exact scope. Existing headless tests are useful regression evidence, not real DPI or clean-OS proof. The app currently has a dark UI; website light/dark themes are a separate requirement.

## Evidence and cleanup

Each step writes a new GUID JSON receipt and installer log; `session.json` is the evolving case ledger and `case.lock` prevents concurrent action races. Receipts distinguish automatic checks, manual assertions, expected failures and untested gates. `Finish` never awards a blanket clean-Windows pass. Review `case-matrix.json` and the concrete step receipts.

Snapshots record only owned process identities and software-rendering flags, runtime/uninstall registration and synthetic sentinel hashes. No raw process command lines are saved. `--disable-gpu --disable-gpu-compositing` flags are configuration evidence, **not measured physical GPU utilization**. Actual GPU/CPU measurements require separate mapped, identity-pinned collectors.

After closing only owned processes, uninstall installed cases, export the redacted receipts and revert the VM snapshot. Reverting the disposable guest is the cleanup boundary, including guest-only credentials/runtime/uninstall entries. The kit deliberately contains no recursive profile deletion, forced process termination, OS-feature enablement, runtime removal or credential manipulation. Retain failed-case evidence before resetting.

## Scope carried from current stabilization

The latest local stabilization already recorded 224 native tests, 12 renderer groups, static package checks, an old **0.9.2 copied helper → current portable candidate** update with actual mounted-React acknowledgement and synthetic sentinels, and isolated Inno repair-gate/native repair cases. These have their own receipts. They do not establish Windows 10/11 clean-image, missing-WebView bootstrap, real production installed migration, fresh/expired-account or full engine-download acceptance. This kit does not republish or rebuild those artifacts.

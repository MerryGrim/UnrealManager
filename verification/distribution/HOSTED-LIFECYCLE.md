# Disposable hosted Server lifecycle

This is a real installation/runtime test, separate from mock UI and source tests. Root publication control remains unchanged. The scripts do not modify a release, publish assets, use a signing secret or contact Epic accounts.

`Build-HostedDistribution.ps1` and `Invoke-HostedLifecycle.ps1` refuse execution unless GitHub's hosted Windows environment variables, a virtual-machine model and a Windows Server OS are observed. Outputs must remain inside a fresh workspace `verification/current` child. The existing clean-desktop kit keeps its stricter Windows 10/11/non-elevated guard; these CI scripts do not weaken it.

The source-acceptance job builds the unsigned packages once on its already provisioned, warm native target and uploads a short-lived same-run artifact. Each matrix label then uses a fresh hosted VM, checks out the same commit and downloads those exact packages; it needs no Node, Rust, compiler, dependency download or signing key:

```yaml
# In acceptance, after all source checks pass and dependencies are provisioned:
# ./verification/distribution/Build-HostedDistribution.ps1 -OutputDirectory "$env:GITHUB_WORKSPACE/verification/current/hosted-packages"
# Upload hosted-packages/Packages as one same-run unsigned test artifact.
lifecycle:
  needs: acceptance
  strategy:
    fail-fast: false
    matrix:
      os: [windows-2022, windows-2025]
  runs-on: ${{ matrix.os }}
  timeout-minutes: 60
  steps:
    # ... checkout this commit; download the same-run artifact directly to
    # verification/current/hosted-packages/Packages (no extra parent folder) ...
    - name: Run actual isolated hosted lifecycle
      shell: pwsh
      run: ./verification/distribution/Invoke-HostedLifecycle.ps1 -PackagesDirectory "$env:GITHUB_WORKSPACE/verification/current/hosted-packages/Packages" -OutputDirectory "$env:GITHUB_WORKSPACE/verification/current/hosted-lifecycle"
    # Retain hosted-lifecycle/reports/** and logs/** plus hosted-packages build
    # report/logs only. Do not upload tools, profiles, fixtures or package EXEs.
```

The build downloads Inno Setup 7.1.0 from its official project release and, only when absent, Microsoft's official Evergreen bootstrapper. Authenticode publisher validation precedes tool execution. Tools, caches, native output and temporary work remain on the workspace volume. The real current installer script and fixed six-file portable layout are used. Packages are unsigned CI fixtures, bound to checkout commit, sizes and SHA-256 in `build-info.json`; they are not a signed update feed or publishable release.

The runtime harness starts only with no existing application process/profile/uninstall registration. It creates inert session/task/project sentinels, disables automatic update checks in that blank profile, chooses English and a normal exit action, and runs:

1. Actual per-user Setup to a unique directory; exact candidate hash and absence of portable marker.
2. Actual native EXE plus WebView. A separate bounded Windows UIAutomation probe must see the real React navigation buttons and app version. Process creation alone is not UI readiness; missing UIA/readiness fails this gate rather than becoming a mocked pass.
3. Normal close with a verified native exit code and observed descendant quiescence; sentinel preservation.
4. Actual native uninstall, profile preservation, reinstall, actual UI again, close and final uninstall. Both uninstaller EXE and DAT hashes and every fixed-AppId registration location are checked before uninstall.
5. Exact portable ZIP extraction, actual UI/readiness/close and marker/Data preservation.

The harness records actual OS ProductType/build/image and whether the hosted token is elevated. It stores no raw command lines, credentials, account content or DPAPI material. WebView root software flags are configuration observations only. A private UIA helper process may be terminated on its own bounded timeout; the app, Setup, uninstaller and unrelated processes are never force-killed. Failed ownership/quiescence checks retain fixture data for VM teardown. Successful cleanup inventories and deletes only roots created from an absent baseline with the matching GUID owner marker; uploaded reports exclude those profiles.

Missing-runtime online bootstrap is reported only if runtime registration and canonical Evergreen executable locations were absent before Setup. If present, the gate is explicitly `NotRun`; the harness never uninstalls the shared runtime or hides registry entries. No firewall, network adapter or proxy is changed to pretend the job is offline.

## Real disk-full case on an owned virtual disk

After source acceptance, `Invoke-HostedDiskFull.ps1` receives the exact library-test executable and SHA-256 from that run's Cargo compiler-artifact JSON. It requires the same disposable hosted Server guard and an elevated token. It creates only a new fixed 64 MiB VHD in its fresh output directory; no existing or physical disk is selected. Before initialization, partitioning and NTFS formatting, the disk is resolved again from that exact image path and checked for capacity, disk identity, non-boot/non-system status and an unused R–Z letter.

The exact ignored native case `engine::install::tests::real_enospc_atomic_preserves_record_and_retries` alone creates the fresh `Case-UUID` directory and bounded filler. It verifies actual Windows error 112/39 during an 8 MiB atomic write, unchanged original record bytes/time, no failed temporary file, successful retry after freeing space and owned fixture cleanup. Its JSON report remains outside the limited volume. The wrapper accepts neither a zero-test run nor a simulated error as success.

Only after the pinned native test and diskpart identities have exited may the wrapper detach the exact image, confirm it detached and remove its own backing file. A timeout or uncertain identity retains the image for VM teardown and reports failure. Unsupported Storage APIs remain an explicit failed/NotRun gate; the scripts do not enable features or install drivers. This is an actual NTFS disk-full test on a virtual disk, not physical storage failure or full-engine installation acceptance. It has not run until its own CI reports prove execution.

## Evidence boundaries and official sources

- [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) are fresh VMs for these standard jobs; Windows jobs run as administrators with UAC disabled. This does not prove a normal standard-user or secure-desktop UAC flow.
- The official image lists identify [Windows Server 2022](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md) and [Server 2025](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md), with preinstalled development software. These are not clean Windows 10/11 consumer images and cannot establish dependency closure without development tools.
- [Microsoft WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) distinguishes the online bootstrapper from the standalone offline runtime. Existing-runtime installation or a repaired registry branch is not absent-runtime/offline acceptance.
- [Inno Setup's official download page](https://jrsoftware.org/isdl.php) links the pinned compiler and identifies its Pyrsys publisher signature.

Clean desktop, genuinely offline missing-runtime, standard-user/UAC, real DPI and fresh/expired Epic acceptance remain separate gates until their own actual evidence exists. Use disposable, supported Windows guest images for these checks. Do not remove shared components from a working desktop or bypass OS requirements to simulate that evidence.

# Acceptance before 1.0

Record the exact candidate SHA-256 and source revision for each run. A local
fixture, a rendered mock, an installed lifecycle and a real account transfer
are separate evidence. Never mark an unexecuted case passed.

| Gate | Repeatable evidence | Remaining acceptance |
| --- | --- | --- |
| Interrupted operations | Native tests cover real child-process termination during APPLY, durable journal/resume, managed-file hashes and unrelated plugin preservation. HTTP fixtures cover interruption, Range resume and bounded corrupt-block retries. | Real Windows permission cancellation; physical storage exhaustion/power loss where required. Injected error 112 is a write-failure check, not a full disk. |
| Installed and portable distribution | Signed package/checksum/ZIP preflight and guarded [disposable guest kit](distribution/README.md). Existing updater fixtures separately exercise helper commit, rollback and repair. | Clean Windows 10 and 11, production Setup install/uninstall/reinstall, missing WebView2 online/offline paths, fresh and legitimately expired Epic sessions in both modes. |
| Repeated verification and resources | [Acceptance command](README.md), locked dependencies, Windows CI definition, nine-locale renderer/DPR/keyboard matrix, owned-process CPU/RAM counters and DXGI-mapped GPU rows. | First actual GitHub CI run after publication of the workflow; native display scaling, long authenticated transfer measurements and sustained resource checks. |

Run the offline acceptance command after dependency provisioning. Keep signed
native updater fixtures opt-in and local; CI needs no signing key. A failed
stage keeps the run failed even if unrelated stages pass. After a fix, retain
the failed report and write the rerun into a fresh directory.

Use `measure-runtime.ps1` on an isolated candidate with no existing manager
process. Its optional phase file labels separately observed idle/tray/login or
transfer work; it does not generate a workload or prove authentication. Run
`classify-runtime-gpu.ps1` on the report immediately on the same boot. Missing
hardware counter rows remain unavailable telemetry, never measured zero.

For native display-scale acceptance use the guest's actual 100%, 150% and 200%
settings and short work areas. Browser deviceScaleFactor checks exercise the
renderer at different pixel densities; they do not change the Windows scale.

Close owned processes, review an exact inventory of temporary fixtures, and
remove only those fixtures. Preserve reports, source, frozen packages, user
projects, download tasks, Vault data and account sessions. Revert disposable
guest snapshots after retaining redacted results. No permanent monitoring or
helper service is required.

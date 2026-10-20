# Change notes

## 0.9.0 — 2026-10-20

This release has no beta suffix. Download the installer or portable edition from
[0.9.0](https://github.com/MerryGrim/UnrealManager/releases/tag/v0.9.0).

- Added a persistent, sequential asset download queue for Vault. Waiting downloads
  can be reordered by drag or keyboard controls; the active item stays fixed.
  A restored queue requires an explicit start. Queue progress stays in memory;
  no service, periodic queue poll or additional runtime process is installed.
- Active downloads appear in Current; waiting items stay in Queue. Completed
  manual recovery removes its interrupted queue entry so later items can proceed.
- Reused Vault content keeps the freshly selected artifact and engine metadata;
  compatibility checks do not silently fall back to an older cached selection.
- Queue requests preserve the chosen asset/version and check account and Vault
  changes before dispatch. Existing partial tasks remain available through the
  explicit resume flow. Copying assets into projects remains a separate action.
- Engine operations check processes belonging to the selected installation.
  An unrelated engine process no longer blocks another engine by name alone;
  an inaccessible live process identity still blocks the operation safely.
- Failure to update the optional protected Epic global list reports a warning
  without rolling back verified engine files. Own registration and the Epic item
  are distinct from full discovery by Epic Launcher. No elevated retry is included.
- Added measured saved-task/block validation, copy/readback, cleanup and deletion
  phases, plus explicit waiting for external prerequisite installers. Percentages
  apply to the current measurable phase.
- Moved the asynchronous cached-hash buffer to heap storage to prevent resume
  dispatch stack growth; avoided unused assembly-plan copies on applying resumes.
- Added native one-time authenticated Epic redemption handoff without exposing
  session tokens to frontend scripts. Earlier remembered-profile verification
  did not activate a key or establish fresh/incognito account acceptance.
- Improved dialog titles, close/back controls and component-list scrolling;
  retained nine interface languages and software rendering for every WebView.
- Preserved the signed installed/portable updater and existing verification key.

Release-cycle checks covered real asset pause/exit/restart/resume, project copy,
isolated engine installation/recovery and Editor launch, queue pause/reorder/
automatic dispatch, deletion and preservation/cleanup. Native tests, mock UI
checks and measurements remain limited to their recorded builds and scenarios.
Clean Windows 10/11 dependency setup, fresh-account SSO, real key activation and
the final version's installed/portable update transition require separate acceptance.

Historical early notes remain in [CHANGELOG-alpha2.md](CHANGELOG-alpha2.md).

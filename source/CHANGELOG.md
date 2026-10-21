# Change notes

## Unreleased — 1.0 stabilization

- Offer explicit restoration of verified old application files after an
  interrupted installed update. Preserve current files, old backups and the
  original journal in a retained repair archive; leave user data untouched.
- Let a new Setup check interrupted-update recovery before copying files,
  including when the installed executable is damaged. Reuse the signed update
  metadata and reject changed backups or unrelated installer claims.
- Acknowledge update startup after the initialized React interface commits,
  retaining compatibility with the existing helper health payload.
- Recover a damaged or out-of-range partial download with one bounded clean
  retry of that block. Keep verified sibling blocks and interrupted transfers.
- Preserve the operating-system cause in Vault filesystem errors and provide
  file-operation context during material updates and rollback.
- Report actual read bytes and progress within a large file during engine and
  material rollback. Show manual engine recovery in the current transfers view.
- Keep keyboard dismissal consistent with disabled close controls during
  material installation, and contain focus within Vault dialogs.
- Remove only the owned partial temporary file when an atomic settings/task
  write fails; retain the previous durable record.
- Add interrupted-APPLY, write-failure and locked-file regression checks, a
  canonical acceptance command, and a separate disposable-Windows distribution
  kit. Development and test tooling remains outside the runtime package.
- Check settings navigation and modal keyboard behavior across all nine
  languages, four renderer pixel scales and two window sizes.
- Show an explicit Epic sign-in action when an account or Fab API returns
  HTTP 401. Keep saved partial downloads and queue order; resume only after
  the user chooses Continue or Retry. CDN errors retain their own meaning.
- Show a manually resumed download once in Current. Keep its durable queue
  binding without displaying the old attention row or counting it twice;
  restore that row if the transfer stops or fails.
- Pass software-rendering arguments directly to all four WebView2 builders,
  including the hidden Epic session window. Retain the locked SDK defaults and
  the environment fallback; elevated hosts can ignore environment overrides.

These changes retain software rendering and use no permanent helper service.
Validation results for this unreleased work are recorded separately from the
published 0.9.3 checks below.

## 0.9.3 — 2026-10-20

Download the installer or portable edition from
[0.9.3](https://github.com/MerryGrim/UnrealManager/releases/tag/v0.9.3).

- Check permissions before long engine preparation and application work,
  including fresh installs, updates and saved-task resumes.
- When the mandatory Epic `.item` registration record needs UAC, use a temporary
  headless helper for that record only. The original application, account
  session and browser profile remain under the original Windows user.
- Preserve the saved task when Windows permission is canceled. Distinguish
  read-only files and sharing violations from access-right failures.
- Report an unsupported protected engine path before long work, with its exact
  location and guidance to select a writable folder or correct permissions.
  The helper does not elevate engine-file operations or change folder ACLs.
- Show access-check and Windows-permission waiting phases. Failure to update
  the optional global Epic list remains a warning, without rolling back an
  otherwise completed engine installation.

The native suite passed 203 tests. One real UAC run applied and restored an
isolated, protected Epic `.item` fixture, with DACL preservation and temporary-file
cleanup checks passing. This does not establish clean-Windows compatibility,
a complete engine installation or an installed/portable update transition.
Other protected-folder and Epic configurations require separate acceptance.

## 0.9.2 — 2026-10-20

Download the installer or portable edition from
[0.9.2](https://github.com/MerryGrim/UnrealManager/releases/tag/v0.9.2).

- Show read progress inside the current file while applying engine updates,
  with actual read bytes and speed, the current check or replacement step,
  and regular status updates while waiting for file I/O.
- Overlap large old/new file checks within one operation on supported non-seeking
  storage. File replacement remains sequential and retains SHA-1 verification,
  file identity checks, protected handles and the existing rollback journal.
- Pause and safe suspension can checkpoint within a large-file check.
- Avoid rewriting an unchanged saved task phase and percentage for heartbeats.

Performance and interrupted-installation compatibility are checked separately
on isolated fixtures. Read speed includes the operating system cache; no
universal SSD speed or fresh real-engine installation is claimed.

## 0.9.1 — 2026-10-20

Download the installer or portable edition from
[0.9.1](https://github.com/MerryGrim/UnrealManager/releases/tag/v0.9.1).

- Streamlined the initial installed-file scan, retaining manifest comparisons,
  SHA-1 verification and the final check before replacing existing files.
- Added bounded storage-aware reading, reusable hash buffers and cancellation
  checks while reading a large file. Unknown or seeking storage uses one reader.
- Show fractional progress, checked files per second, bytes read and the current
  file without inventing a total byte count or an estimated completion time.
- The official Epic redemption page follows the selected application language.

Performance and runtime results belong to the corresponding verification
reports; no general speed multiplier or 1.0 compatibility claim is implied.

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

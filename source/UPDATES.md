# Signed Windows updates

The application uses a detached ECDSA P-256/SHA-256 signature over the exact
UTF-8 bytes of `update-manifest.json`. `update-manifest.sig` contains the
base64-encoded 64-byte IEEE P1363 signature. The public verification key is
compiled from `src-tauri/update-public-key.json`. There is no signing key,
background update service or development runtime in the distributed app.

Version 0.9.13 requires one manual upgrade from 0.9.12 or older. The previous
private update-signing key was lost after a Windows reinstall. Earlier apps
cannot verify a replacement key, so use the 0.9.13 Setup in the existing
installation location, or replace the portable application files while keeping
`Data` and `portable.marker`. The existing profile is preserved.

Version 0.9.13 embeds a new public verification key for future releases. Its
GitHub release deliberately omits `update-manifest.json`, `update-manifest.sig`
and `.umupdate`; older updaters therefore skip this manual release. From 0.9.13,
**Settings → App updates** retains stable/beta channels, manual checks and signed
download/apply for subsequent compatible releases, beginning with 0.9.14.
Changing the key does not make a new signature acceptable to an earlier app.

Version 0.9.13 discovers engines left on fixed local disks after a Windows
reinstall and accepts an existing folder for the exact available build. It
verifies selected files, installs missing components and preserves unrelated or
conflicting files. Manual folder adoption works without an Epic session; file
verification and downloads use the available Epic build. Stable and preview
engine groups are separate, with unconfirmed channels labeled explicitly.
Prerequisite receipts from another Windows installation no longer skip checks.
The manual installed/portable upgrade preserves existing data folders. See
[behavior and limits](ENGINE-DISCOVERY.md).

Version 0.9.12 centers the main window on startup and opens key activation in a
wizard within the existing application WebView: one key, a batch/TXT list or the
official Epic page. Product/account review, explicit confirmation, ready-only
activation and native pacing/recovery remain. The existing separate official
Epic window and current session are reused. That release retained the previous
signing identity and existing data folders. The single-key
mode uses a one-line field and rejects pasted or dropped lists.

The wizard can explicitly export already-used keys and keys for products already
owned to a chosen local TXT, with full keys, product names and separate reasons.
Owning the product does not prove that its key was used. Full keys stay in the
native operation and are written only to the selected file for this export;
settings and application history do not retain them.

Version 0.9.11 restores `CODE_PLUGIN` and classifies owned Unreal Engine content
from Fab metadata and Epic package descriptions before downloading, rather than
relying only on local Vault packages. Verified type results are cached;
synchronization checks new, changed or expired records with visible progress.
Failed metadata lookups retain the
library and show an unresolved count with a retry notice. Type checks do not
download asset files or chunks and add no permanent process.

The unpublished 0.9.10 candidate's selected-manifest safeguards remain: saved
Vault packages are reused, and a corrected full project requires a fresh,
explicitly selected destination. Conflicting files are not overwritten.
Optional replacement with confirmation and a backup remains future work.
An existing local 0.9.10 installation uses the same one-time manual upgrade to
0.9.13 while retaining its profile. The previous 0.9.11/0.9.12 release line used
the original signing identity.

Version 0.9.9 fixed batch key checks when Epic reports `content_already_owned`.
That row is shown as already owned, checking continues, and explicit activation
includes only ready rows. Unknown activation results are still not replayed.
Those older updates retained their signing identity and installed/portable data
folders without requiring a new profile or a manual sign-in.

## Preparing release assets

The 0.9.3 engine permission helper is separate from the signed application
updater. It requests UAC only for a mandatory Epic `.item` record and then exits;
it does not restart the application as administrator, migrate an Epic session,
change engine-folder permissions or install a permanent service. Canceling the
permission prompt preserves the saved engine task. Protected engine files must
remain writable by the original user. The optional global Epic list can still
produce a warning rather than full Epic Launcher discovery.

Version 0.9.6 also supports an explicit engine-registration check that can request
a temporary headless UAC broker for the fixed shared Epic
`%ProgramData%\Epic\UnrealEngineLauncher\LauncherInstalled.dat` metadata file.
This is a separate, user-initiated reconciliation of engine records; it does not
run the application as administrator, change engine files or repair folder ACLs.
The normal user profile and Epic session remain with the original process.
Automatic engine installation still treats the optional shared list as a warning;
successful local registration alone does not guarantee full Epic Launcher discovery.

For 0.9.3, 203 native tests passed, and one real UAC run applied and restored an
isolated protected Epic `.item` fixture. DACL preservation and temporary-file
cleanup checks passed. This verifies that limited registration-helper scenario;
it does not establish a complete engine installation, clean-Windows compatibility
or an installed/portable application-update transition.

Use Windows and PowerShell 7. Reuse the new signing identity established by
0.9.13 for subsequent releases; do not create another replacement for each
release. The 0.9.13 key change is an explicit migration after loss of the previous
private key, with the manual upgrade described above. Keep the private key in
private, protected storage and retain a recoverable backup. Never upload a
private key or its DPAPI file. Replacing the public key again would require
another migration for existing installations.

Build the release EXE and Inno installer into a fresh staging directory, with
the portable distribution in its `UnrealManager-Portable` subdirectory. Then:

```powershell
.\PREPARE-UPDATE.ps1 -ReleaseDirectory '..\Releases\0.9.14\my-release'
```

Use `-PortableDirectory` for a different clean portable distribution folder,
or `-PrivateKeyPath` for the matching encrypted signing key in another private
location. The script requires all source version declarations, the app EXE
and the installer product metadata to agree. Existing update outputs cause a
failure; choose a new staging directory instead of replacing published files.

For signed releases after the manual-only 0.9.13 migration, publish these assets
together on the matching `v<version>` GitHub Release in
`MerryGrim/UnrealManager`:

For future normal non-prerelease SemVer releases, use the stable manifest channel
and the signing identity embedded in 0.9.13. Existing beta-channel preferences
remain valid; that channel also includes stable releases. Both channels require
a strictly newer version and matching verified release assets before an update
is offered. The manual-only 0.9.13 release is the exception: preparation outputs
may be checked locally, but its update manifests and `.umupdate` are not published.

- `UnrealManager-<version>-Windows-x64-Setup.exe`
- `UnrealManager-<version>-Windows-x64-Portable.umupdate`
- `update-manifest.json`
- `update-manifest.sig`

Continue publishing the ordinary portable ZIP, dependency source archive and
checksums for manual downloads and license compliance. The `.umupdate` file is
for the application updater; users do not launch it directly.

## Portable payload and validation

The `.umupdate` asset is a gzip-compressed UTF-8 JSON object with exactly
`schema`, `version` and `files`. Each file has exactly `name` and base64 `data`.
The payload contains five owned distribution files: `unreal-manager.exe`,
`README.txt`, `LICENSE`, `THIRD_PARTY_NOTICES.md` and
`THIRD-PARTY-LICENSES.txt`. It excludes `portable.marker`, `Data`, sessions,
downloads and user projects. The input folder may additionally contain a
portable marker and `RELEASE-INFO.json`; other inputs are rejected.

The manifest binds version, channel, Windows x64 target, application SHA-256,
and the fixed release URL, size and SHA-256 of each installed/portable payload.
The preparation tool checks file bounds and verifies its signature with the
tracked public key before creating outputs. The app verifies the signature and
payload again before applying an update. Signing checks prove artifact
integrity; native update and recovery behavior require their own verification.

## Interrupted installed updates (current source)

The recovery flow described here is implemented in the current source. It does
not retrofit these commands into already distributed 0.9.3 executables.

An installer that stops during file replacement can leave output whose hashes
were never recorded. Startup does not guess that those files are safe to
overwrite automatically. If all five old backups match their recorded hashes,
the current application offers an explicit, default-No recovery confirmation.
Before changing anything, it copies and verifies every existing owned
application file into a unique `repair-<uuid>/current` archive under
`.um-app-update-<update-id>`. Missing files are recorded as missing. It then
restores the verified old files. The old backups, current-file archive and
original journal remain available as evidence. The active journal is moved
into that archive only after all restored files match the old hashes.

Recovery is limited to `unreal-manager.exe`, `README.txt`, `LICENSE`,
`THIRD_PARTY_NOTICES.md` and `THIRD-PARTY-LICENSES.txt`. It does not modify
`Data`, portable markers, Epic sessions, downloads, projects or other files.
It refuses changed inputs and damaged/missing backups. A failure can leave
some owned files restored and others unchanged; all backups and repair
evidence remain. Retry requires a fresh explicit confirmation. Do not delete
the journal or backup directory to bypass a recovery error.

If the installed EXE cannot run, close Unreal Manager and use a trusted new
portable EXE that contains this recovery command. Run it as the original
Windows user, without elevation:

```powershell
& 'C:\path\to\new-portable\unreal-manager.exe' --repair-app-update `
  "$env:LOCALAPPDATA\Programs\Unreal Manager\.um-app-update.json"
```

Use the actual installation folder if it differs from the example. The command
checks the retained signed manifest, installer payload, journal and old backups;
after approval it restores the old application and starts it. A new Setup also
uses its own extracted EXE to perform this check before copying application
files. It does not start the old app during Setup. A normal signed auto-update
whose exact live helper owns that installer continues through the gate without
a repair prompt. An unrelated installer or process cannot claim that bypass.

This flow requires a readable intact journal, signed staging metadata and
verified old backups. If those are missing, recovery stops with retained
evidence; it cannot reconstruct a missing backup or a destroyed executable
without a separate trusted download. It does not repair uninstall registration
or other installer side effects outside the five owned files.

## Update health acknowledgement (current source)

The new app acknowledges an update once React has successfully committed the
main interface, after local preferences and interface language initialization
have succeeded. Native window creation alone no longer acknowledges success.
A failed preference load or failed initial render leaves the helper waiting;
the existing bounded health timeout then follows its rollback rules. Optional
network update checks and drive scans do not block this signal.

The acknowledgement retains the existing `id`, `version` and `appSha256`
payload, so an older 0.9.3 helper can accept a new app's signal. This is a
startup readiness check, not proof that every later screen or network operation
works. The helper remains a one-shot process; no service or persistent monitor
is installed.

Run the preparation regression after a native build:

```powershell
pwsh -NoProfile -File ..\verification\test-prepare-update.ps1
```

It copies an existing native EXE into a unique ignored verification directory,
creates ephemeral test signing keys and checks signatures, tampering, bundle
contents, version mismatches and overwrite refusal. Use `-FixtureExecutable`
for a build in a custom target directory. Its Setup input is a fixture copy;
the regression never launches a binary or runs a real installer/update.

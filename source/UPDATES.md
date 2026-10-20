# Signed Windows updates

The application uses a detached ECDSA P-256/SHA-256 signature over the exact
UTF-8 bytes of `update-manifest.json`. `update-manifest.sig` contains the
base64-encoded 64-byte IEEE P1363 signature. The public verification key is
compiled from `src-tauri/update-public-key.json`. There is no signing key,
background update service or development runtime in the distributed app.

In 0.9.3, open **Settings → App updates** to select the stable/beta channel,
check, download and apply an update. Beta.7 introduced the updater; beta.7 and
newer can receive 0.9.3 without a separate manual installation. The stable
0.9.3 release is available in both update channels. In beta.7, use
**Settings → Unreal Manager updates → Check for updates**.
Beta.6 requires one manual upgrade to beta.7 or newer.

## Preparing release assets

The 0.9.3 engine permission helper is separate from the signed application
updater. It requests UAC only for a mandatory Epic `.item` record and then exits;
it does not restart the application as administrator, migrate an Epic session,
change engine-folder permissions or install a permanent service. Canceling the
permission prompt preserves the saved engine task. Protected engine files must
remain writable by the original user. The optional global Epic list can still
produce a warning rather than full Epic Launcher discovery.

For 0.9.3, 203 native tests passed, and one real UAC run applied and restored an
isolated protected Epic `.item` fixture. DACL preservation and temporary-file
cleanup checks passed. This verifies that limited registration-helper scenario;
it does not establish a complete engine installation, clean-Windows compatibility
or an installed/portable application-update transition.

Use Windows and PowerShell 7. Reuse the existing signing key for this repository;
do not create a replacement for a new release. `CREATE-UPDATE-KEY.ps1` is only for
initial key creation before the first deployment of a distinct application. The private key is
encrypted for the current Windows account with DPAPI and stored under the
ignored `.release-secrets` directory. Keep that key; replacing the public key
requires a separate migration for existing installations. Never upload the
private key or its DPAPI file.

Build the release EXE and Inno installer into a fresh staging directory, with
the portable distribution in its `UnrealManager-Portable` subdirectory. Then:

```powershell
.\PREPARE-UPDATE.ps1 -ReleaseDirectory '..\Releases\0.9.3\my-release'
```

Use `-PortableDirectory` for a different clean portable distribution folder,
or `-PrivateKeyPath` for the matching encrypted signing key in another private
location. The script requires all source version declarations, the app EXE
and the installer product metadata to agree. Existing update outputs cause a
failure; choose a new staging directory instead of replacing published files.

Publish these assets together on the matching `v<version>` GitHub Release in
`MerryGrim/UnrealManager`:

0.9.3 is a normal non-prerelease SemVer version: use the stable manifest channel
and the existing signing key. Existing beta-channel preferences remain valid;
that channel also includes stable releases. Both channels require a strictly
newer version and the matching verified release assets before an update is offered.

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

Run the preparation regression after a native build:

```powershell
pwsh -NoProfile -File ..\verification\test-prepare-update.ps1
```

It copies an existing native EXE into a unique ignored verification directory,
creates ephemeral test signing keys and checks signatures, tampering, bundle
contents, version mismatches and overwrite refusal. Use `-FixtureExecutable`
for a build in a custom target directory. Its Setup input is a fixture copy;
the regression never launches a binary or runs a real installer/update.

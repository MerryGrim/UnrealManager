# Architecture — 0.2.0-beta.6

## Desktop interface

React and TypeScript run in Tauri's system WebView2 window. The interface covers
engines, projects, the Fab library, plugins, content updates, downloads and
settings. Russian text and eight translated catalogues are available locally.
The main window invokes Rust commands; remote Epic windows cannot invoke them.

## Native core and Epic integration

Rust owns filesystem scans, process launch, downloads, manifests, hashing and
installation. `egs-api` and `reqwest` provide Epic/Fab access. Login and key
redemption open official Epic pages in separate WebView2 windows; account tokens
are stored using Windows DPAPI and are not sent to the application interface.
Epic/Fab endpoint compatibility can change independently of this application.

Engine installation uses selected components, staging, integrity checks and a
recovery journal. Vault content can be downloaded, copied into projects or
installed as plugins; project updates preview conflicts before applying changes.
Engine and Vault tasks retain exact versions and resume information. Completed
chunks are reused, and interrupted HTTP chunks can resume when the server
supports Range requests. Resumption after restarting is initiated by the user.

## Local storage

Storage uses JSON metadata, compressed task snapshots and files on disk; there
is no SQLite index in this version. Installed builds use
`%LOCALAPPDATA%\UnrealManager\Data`. Portable builds detect `portable.marker`
beside the executable and use the adjacent `Data` directory. Development builds
have a separate root selected by `storage.rs`.

Settings, account data, cached catalogues and WebView profiles have separate
locations. A Vault directory can be chosen independently. Saved engine/Vault
task metadata survives cache clearing; content remains subject to its own Epic
or publisher terms rather than the application's source license.

## Resource policy

`main.rs` sets `--disable-gpu --disable-gpu-compositing` before creating the main
or Epic WebView2 windows. This configures software rendering; actual GPU activity
must be measured on each computer, separately from Windows desktop composition.
The interface has no WebGL, canvas animation or idle rendering loop. Loading
indicators animate during operations and respect reduced-motion settings.

Backend progress updates are throttled. Library pages limit rendered tiles;
intersection observers defer cover requests, and cached covers are resized
before display. Hidden sections use `display:none`. Development and test tools
are not application runtime dependencies.

## Distribution and verification

The Windows executable uses system WebView2. Portable packaging and the optional
Inno Setup installer are separate build steps. `GENERATE-ICONS.ps1` recreates
the original geometric UM artwork using CPU-based Windows drawing APIs.

Application self-updates are planned only: `app_updates.rs` is an unconnected
draft, while the current Updates view handles Vault content. No update server
is part of the application.

Rust tests and mock UI regressions check their respective code paths. A mock
result does not establish that an authenticated Fab transfer, full Unreal Engine
installation or native GPU measurement succeeded; see `../verification/README.md`.

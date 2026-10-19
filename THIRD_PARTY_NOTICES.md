# Third-party components and trademarks

The root MIT license covers original Unreal Manager code, documentation, and the
neutral UM artwork in this repository. It does not relicense dependencies or
their copyright and license notices.

## Dependencies

- Frontend dependencies and exact versions are recorded in
  `source/package-lock.json`; native dependencies in `source/src-tauri/Cargo.lock`.
- `source/src/licenses.json` preserves collected dependency metadata and original
  license texts and is displayed in the application's **About → Licenses** tab.
  All 560 entries in the beta.8 snapshot have license texts. The inventory includes
  build-time and other-platform packages; it does not imply that every component
  is distributed in the Windows executable or licensed under MIT.
- Both binary editions include `THIRD-PARTY-LICENSES.txt` with the full collected
  license texts, copyright notices, exact versions and source URLs.
- Main dependencies include React (MIT), Tauri (MIT OR Apache-2.0), Lucide
  (ISC, including Feather attribution), and egs-api (MIT, acheta-games).
- Some transitive Rust packages use MPL-2.0, including cssparser,
  cssparser-macros, dtoa-short, option-ext, and selectors. Their original licenses
  continue to apply. Unmodified source packages matching Cargo.lock checksums
  are attached to [beta.8 as ThirdPartySources.zip](https://github.com/MerryGrim/UnrealManager/releases/download/v0.2.0-beta.8/UnrealManager-0.2.0-beta.8-ThirdPartySources.zip).
  The archive includes exact versions, SHA-256 values, original license notices
  and extraction instructions. The same sources are available via the exact
  <https://crates.io/> URLs in the notices.
- `verification/generate-licenses.py` validates the inventory against both lock
  files, checks cached crate archives, and records upstream license provenance.
  It is development tooling. Review and regenerate notices and corresponding
  source packages when dependencies change for a future binary release.

## Microsoft WebView2

WebView2 is a Microsoft runtime and is not covered by this project's MIT license.
The Microsoft bootstrapper is deliberately excluded from this source repository.
For installer builds, download the official Evergreen Bootstrapper from
<https://developer.microsoft.com/microsoft-edge/webview2/> to
`source/installer/dependencies/MicrosoftEdgeWebview2Setup.exe`.
`BUILD-INSTALLER.ps1` verifies its Microsoft Authenticode signature before use.
Microsoft's redistribution terms apply to that file and to WebView2.

## Unreal Engine, Epic Games and Fab

Unreal Engine, Epic Games, Fab and related product names and marks belong to
their respective owners. Unreal Manager is an independent project, not affiliated
with or endorsed by Epic Games. The MIT license grants no trademark rights.

The published UM application artwork is original geometric lettering; it does
not contain the Unreal Engine logo. This repository does not distribute Unreal
Engine source code, engines, paid assets, Vault content, or account sessions.
Access to Epic/Fab services and licensed content remains subject to the user's
account permissions and the relevant service and content licenses.

## Website screenshots

Screenshots under `docs/assets/screenshots` show the real frontend rendered with
synthetic demonstration data. They contain no account session, purchased asset
artwork, or private project data. They illustrate the interface and do not prove
a successful real Epic/Fab transfer or full Unreal Engine installation.

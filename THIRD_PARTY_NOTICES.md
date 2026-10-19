# Third-party components and trademarks

The root MIT license covers original Unreal Manager code, documentation, and the
neutral UM artwork in this repository. It does not relicense dependencies or
their copyright and license notices.

## Dependencies

- Frontend dependencies and exact versions are recorded in
  `source/package-lock.json`; native dependencies in `source/src-tauri/Cargo.lock`.
- `source/src/licenses.json` preserves collected dependency metadata and original
  license texts and is displayed in the application's **About → Licenses** tab.
  It is an existing snapshot, not a claim that all dependencies are MIT or that
  every build-time package is distributed in the application.
- Main dependencies include React (MIT), Tauri (MIT OR Apache-2.0), Lucide
  (ISC, including Feather attribution), and egs-api (MIT, acheta-games).
- Some transitive Rust packages use MPL-2.0, including cssparser,
  cssparser-macros, dtoa-short, option-ext, and selectors. Their original licenses
  continue to apply. Sources are available from the exact crate versions
  identified by Cargo.lock via <https://crates.io/>.
- Dependencies are not vendored here. Before distributing a new binary, review
  notices against the resolved dependency graph, include required license texts
  and attributions, and satisfy applicable source-availability requirements for
  components included in that binary. Entries without collected texts in the
  existing snapshot must be resolved for the corresponding distributed component.

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

Unreal Manager 0.9.6 — Nyanma
https://github.com/MerryGrim

Release information, downloads and signed application updates:
https://github.com/MerryGrim/UnrealManager/releases

Installed edition
The installer installs for the current Windows user. Settings, account session,
cache and downloads are kept in %LOCALAPPDATA%\UnrealManager\Data.
Uninstalling the application preserves this data folder.

Portable edition
Keep portable.marker beside unreal-manager.exe. Data is stored in the Data folder
beside the executable. Use a writable folder and move the whole folder together.
The Epic session is protected by Windows DPAPI; sign in again on another PC/user.
Microsoft WebView2 Runtime must be installed. The Inno installer installs it if
missing; its bootstrapper needs Internet access.

Rendering and runtime requirements
WebView2 uses software rendering: GPU acceleration and GPU compositing are
disabled before the main and Epic windows are created. No Node.js, Rust,
Visual Studio, Inno Setup or Playwright installation is required to run the EXE.
Actual GPU activity must be measured on each computer; Windows desktop
composition is separate from the application's rendering.

In Settings > Storage, Clear cache removes catalogue metadata and cached images only.
It preserves preferences, Epic sign-in, Vault downloads, projects and engines.
The cache can be rebuilt when the application needs it.

Распакованная версия: запускайте unreal-manager.exe, оставив рядом portable.marker.
Настройки и кэш находятся в Data рядом с программой.
Установленная версия хранит данные в %LOCALAPPDATA%\UnrealManager\Data.
Удаление программы через деинсталлятор сохраняет эту папку данных.
Очистка кэша в настройках не удаляет вход в Epic, настройки и скачанные материалы.

The EXE has no Windows Authenticode publisher signature. Update manifests have
a separate ECDSA signature verified with the application's pinned public key. Original Unreal Manager source code
is licensed under the MIT License. Third-party components retain their
respective licenses; notices are available in Account menu > About and in
THIRD-PARTY-LICENSES.txt. Corresponding MPL component sources are provided in
the ThirdPartySources archive alongside
the distribution. Published source archives are also listed in GitHub Releases:
https://github.com/MerryGrim/UnrealManager/releases
Unreal Engine, Epic Games and Fab belong to their owners.

Installer tooling: Inno Setup 7.1.0, https://jrsoftware.org/isdl.php
WebView2 distribution: https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution

0.9.6
- Projects now has separate Refresh list and Find projects actions. Choose a local
  folder or drive for discovery; visible counters and the current folder show progress.
- Stop ends traversal and saves the valid projects already found. Exact project
  paths are remembered between launches. Skipped entries and bounded-search limits
  are reported; project files are never modified by discovery. No watcher is added.

0.9.5 (historical release)
- Repairing managed engine registration can request Windows administrator approval
  for protected Epic metadata. The main application keeps its ordinary token.
- Projects created from Fab are remembered by their exact .uproject path and appear
  after creation in the local Projects view, on any selected drive or folder.
- Library sorting: name, type, newest supported UE, and downloaded state.
  Name and UE sorts support both directions; page size sits beside bottom navigation.
  Acquisition/update dates and download size are not supplied by this library view.
- Native browser context menus are suppressed on empty main-window surfaces;
  text editing and application card menus remain available.

0.9.2 (historical release)
- Progress inside each file during engine update application, including read bytes,
  read speed and the current check or replacement step.
- Bounded concurrent old/new file checking on suitable SSDs. Replacements stay
  sequential; verified files stay protected until their replacement step.
- Compatible saved installation tasks and the existing rollback journal format.

0.9.1
- Streamlined installed-file verification with bounded storage-aware reading.
- Fractional progress, checked files per second, read bytes and current file.
- The official Epic redemption window follows the application's selected language.

0.9.0
- Sequential asset downloads to Vault; reorder pending items by drag or keyboard.
- Restored queues start paused. Enqueue does not prepare or alter an active task.
- Project copying stays a separate explicit action; old partial tasks are preserved.
- Engine guards check the selected installation, not every running engine version.
- Optional protected Epic global registration reports a warning without undoing
  verified engine files. An elevated registration retry is not implemented.
- Real counters for saved blocks, file copy/readback, cleanup and deletion;
  external system-component installation has an explicit waiting state.
- Fixed resume dispatch stack growth and dialog navigation/scrolling.
- Existing signing key and installed/portable update mechanisms are preserved.
- Active downloads appear in Current; the queue contains waiting items.
- Verified scenarios include asset pause/restart/resume, project copying,
  isolated engine installation/recovery, Editor launch and test-data cleanup.
  Results cover the recorded builds and operations. Clean Windows, installed/
  portable update migration, fresh-account SSO and real key activation are
  separate checks.

0.2.0-beta.8 (historical release)
- Settings grouped into Interface, Downloads, Storage, Projects and App updates.
- Open Settings > App updates to check, download and apply application updates.
- Category labels are translated into all nine interface languages.
- The same lightweight runtime and software-rendering policy are preserved.

0.2.0-beta.7 (updater introduction)
- GitHub application updates for installed and portable editions.
- Settings: stable/beta, automatic startup check at most daily, manual check.
- Separate download and apply actions; signed manifest and SHA-256 verification.
- Temporary apply helper exits after restart; no permanent service or polling timer.
- Portable updates preserve Data, portable.marker and unknown user files.
- Installed updates reuse the existing installation directory.
- File backups support rollback if apply or native startup confirmation fails.
- Public documentation and examples use generic or synthetic paths.

In 0.9.6, use Settings > App updates > Check for updates.
Beta.7 and newer can receive 0.9.6 through the application updater. Stable
releases are available in both the stable and beta update channels.
In beta.7, use Settings > Unreal Manager updates > Check for updates.
Beta.6 needs one manual upgrade to beta.7 or newer to receive this updater.
Before applying, active operations stop safely and their resumable tasks persist.
If an Inno installer is interrupted mid-write, recovery keeps its backup and
reports an error rather than overwriting files whose provenance is uncertain.

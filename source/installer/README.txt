Unreal Manager 0.2.0-beta.8 — Nyanma
https://github.com/MerryGrim

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
THIRD-PARTY-LICENSES.txt. Corresponding MPL component sources are attached to
https://github.com/MerryGrim/UnrealManager/releases/tag/v0.2.0-beta.8
Unreal Engine, Epic Games and Fab belong to their owners.

Installer tooling: Inno Setup 7.1.0, https://jrsoftware.org/isdl.php
WebView2 distribution: https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution

0.2.0-beta.8
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

In beta.7, select Stable and beta releases, then use
Settings > Unreal Manager updates > Check for updates to receive beta.8.
Beta.6 needs one manual upgrade to beta.7 or newer to receive this updater.
Before applying, active operations stop safely and their resumable tasks persist.
If an Inno installer is interrupted mid-write, recovery keeps its backup and
reports an error rather than overwriting files whose provenance is uncertain.

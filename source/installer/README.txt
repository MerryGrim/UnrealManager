Unreal Manager 0.2.0-beta.6 — Nyanma
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

In Settings, Clear cache removes catalogue metadata and cached images only.
It preserves preferences, Epic sign-in, Vault downloads, projects and engines.
The cache can be rebuilt when the application needs it.

Распакованная версия: запускайте unreal-manager.exe, оставив рядом portable.marker.
Настройки и кэш находятся в Data рядом с программой.
Установленная версия хранит данные в %LOCALAPPDATA%\UnrealManager\Data.
Удаление программы через деинсталлятор сохраняет эту папку данных.
Очистка кэша в настройках не удаляет вход в Epic, настройки и скачанные материалы.

The application is not digitally signed. Original Unreal Manager source code
is licensed under the MIT License. Third-party components retain their
respective licenses; notices are available in Account menu > About and in
THIRD-PARTY-LICENSES.txt. Corresponding MPL component sources are attached to
https://github.com/MerryGrim/UnrealManager/releases/tag/v0.2.0-beta.6
Unreal Engine, Epic Games and Fab belong to their owners.

Installer tooling: Inno Setup 7.1.0, https://jrsoftware.org/isdl.php
WebView2 distribution: https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution

0.2.0-beta.6
- Vault materials, projects and plugins support pause and resume after restart.
- Exact package versions and selected project actions survive closing the app.
- Completed chunks are reused; interrupted chunks resume with HTTP Range when supported.
- Engine installations can also resume after a safe stop on closing the app.
- Engine downloads show progress and current speed in Downloads.
- Settings include a shared download limit for engines and Vault content (MB/s; 0 = unlimited).
- The close button can ask, minimize to the system tray, or exit; the choice is changeable in Settings.
- Engine removal requires confirmation and then removes the entire selected engine folder permanently.

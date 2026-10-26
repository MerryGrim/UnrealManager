# Unreal Manager website

Buildless static GitHub Pages site. Publish this directory with the repository's Pages workflow; no npm install or build is required.

- `index.html`: accessible page structure and English fallback copy.
- `translations.js`: all nine application languages; English interface strings are read from the HTML at startup.
- `app.js`: language and theme preferences, accessible System/Light/Dark segmented buttons, metadata, and shareable `?lang=ru&theme=light#features` links.
- `styles.css`: responsive dark and light themes, with no animation or external fonts.
- `assets/um-mark.svg`: original project mark.
- `assets/screenshots/*.webp`: the actual React interface with fictional demonstration data and English locale. The original 0.9.0 gallery shows the earlier dark appearance; the new 0.9.8 batch-redemption set includes dark and light themes. Screenshots do not verify real Epic/Fab transfers or activations. See the repository's `THIRD_PARTY_NOTICES.md` for scope and attribution.

The website does not include analytics, external requests, a framework, or a package manager runtime. Language and theme are saved in browser local storage when available. The system theme follows the operating system's preference. All copy, image descriptions, control labels, and page metadata are translated.

Serve this directory using any local static HTTP server. For example, when Python is already available:

```powershell
python -m http.server 4173 --bind 127.0.0.1 --directory docs
```

Open `http://127.0.0.1:4173/`. Browser QA is covered by `verification/test-site.cjs` at repository root. Its requirements are development tools only.

The page links to the Windows x64 installer and portable ZIP for `v0.9.10`, alongside release notes and SHA-256 checksums. Source and build instructions remain available as secondary actions. Publish the matching GitHub release assets before deploying a website revision with new download links.

The page describes 0.9.10's sequential download-to-Vault queue, pending-item drag/keyboard priorities, explicit restart behavior, measured phase progress and optional Epic-registration warnings. Project copying remains a separate action. It also describes signed GitHub app updates for installed and portable copies, startup checks at most once daily, manual checks and a temporary apply helper. This website never performs an application update. Software-rendering configuration is distinguished from measured GPU activity. Demonstration screenshots are illustrative and do not establish clean-Windows compatibility or acceptance of every workflow.

`verification/capture-public-screenshots.cjs` uses the real frontend with a fictional account, self-authored covers, synthetic `X:/Fixture/Demo` paths and simulated queue progress. It blocks external requests and never starts the native app. Its `--preview` flag writes fresh private preview images instead of website assets. The retained 0.9.0 gallery shows the earlier dark app appearance. The separate batch-redemption section uses fresh 0.9.8 renderer fixtures in dark and light themes; website theme controls do not alter screenshot pixels.

Version 0.9.2 adds bounded parallel checks of large files while applying engine updates, with a separate current-file meter, old/new file checking stages and explicit replacement status. Actual application read bytes and read speed stay separate from completed logical output. Read metrics may include OS cache; they are not physical SSD benchmarks. No universal speed multiplier or ETA is claimed. Retained demonstration screenshots were captured with version 0.9.0 and illustrate the shared interface; they are not new runtime acceptance evidence for 0.9.2.

Version 0.9.3 checks rights before long fresh-install, update and resume work. A temporary headless UAC helper can handle only a mandatory Epic `.item` record; the original application and Epic session remain in the user profile. Canceling preserves the saved task. Protected engine folders require a writable location or corrected permissions, with read-only and sharing errors distinguished. The helper does not grant general engine write access or repair ACLs. Optional global Epic registration remains warning-only. These feature descriptions do not establish live UAC or clean-Windows acceptance for this version; the retained screenshots also do not establish 0.9.3 runtime acceptance.

Version 0.9.4 describes interrupted app-update recovery and completion acknowledgement after the new React interface initializes. Download recovery checks damaged partial blocks before fetching them again; checking, copying and rollback expose measured progress. A manually resumed queue task appears once while active, without deleting its persistent binding before success. Expired Epic authorization offers an explicit sign-in action while preserving tasks and the queue. Software-rendering arguments are supplied through all four WebView window-construction paths, including Epic sign-in and redemption. These are configuration claims, not measurements of zero GPU activity.

Version 0.9.5 adds seven local library sort orders and a page-size control below the results, records successfully created projects independently of their folder without a watcher, and suppresses blank-area context menus while preserving editable fields and custom menus. The explicit engine-registration check may request a temporary headless UAC broker only for Epic's fixed shared `LauncherInstalled.dat`; it does not elevate the application, engine-file writes or folder-permission changes. Date/size ordering is not included. New broker acceptance is recorded separately from previous release evidence.

The retained 0.9.0 demonstration screenshots remain unchanged. Local headless website checks cover nine languages, two themes, responsive layouts, keyboard controls and download links. Hosted Windows Server lifecycle results are distinct from clean Windows 10/11 desktop acceptance, standard-user UAC testing and physical GPU measurements; this page makes no 1.0-readiness or new human-UAC acceptance claim.

The 0.9.5 landing-page layout introduced a wide demonstration preview, blue project accents, feature panels and a larger screenshot gallery. Layout and theme controls remain static and keyboard accessible; there are no animations, external assets or new runtime dependencies. That revision labeled its hero demonstration image as version 0.9.0; the later batch-focused hero has its own 0.9.8 demonstration label.

Version 0.9.6 separates refreshing the known project list from a finite search in an explicitly selected local folder or drive. Search shows directory/project counts and the current folder, supports Stop, and remembers paths found before completion or Stop without adding a watcher. Registration failures, skipped entries and partial results are reported. This describes the feature; website tests and retained demonstration screenshots do not establish real user-drive discovery acceptance.

Version 0.9.7 adds built-in single-key and TXT batch input, a product/account preview and a separate explicit activation confirmation. The scrollable result list has exactly three columns: the final key block (or masked suffix), product title, and status with a reason. Native events expose a fixed two-second pause between keys and Stop after the current key; uncertain outcomes are not automatically replayed. The official Epic window is separate and receives no input keys from the app. Keys are transient operation data, not settings or history.

Project scan folders can be configured and remembered. Refresh searches those folders and their subfolders with progress and Stop, while successful completion uses a brief notification. The old project text-filter/long help block is no longer part of that page; removing a scan folder does not delete files. These are source feature descriptions, not proof of real Epic batch redemption, consumer Windows compatibility or zero GPU activity.

The retained version 0.9.0 demonstration screenshots do not depict the new 0.9.7 key-batch view or project-folder configuration dialog. Nine-language/two-theme website checks remain separate from native account acceptance.

Version 0.9.8 brings navigation, account, theme switching and native window actions into one integrated header, without a separate native title bar. Minimize/maximize/close keep the existing window APIs and Ask/Tray/Exit choice. Light and dark application themes are saved and selectable from the header or Interface settings; the initial theme is restored before mounting, with a safe dark fallback. Window updates are event-driven, without idle polling or a permanent helper.

The existing gallery remains explicitly marked as version 0.9.0 demonstration UI. It does not show the new 0.9.8 header or light application theme, and no image has been relabeled as current runtime proof. Nine-language/two-theme website checks validate the static page only; native window interactions, real Epic flows, clean desktop Windows and physical GPU activity remain separately scoped evidence.

The batch-redemption section added after the 0.9.8 release emphasizes a list → preview → explicit confirmation workflow. It accepts up to 100 unique keys or a UTF-8 TXT file up to 64 KiB. The three-column table keeps only the displayed key suffix/final block, product title and status with a reason. Already-used keys do not stop checks of later rows; activation includes only ready rows.

The application spaces ordinary requests by two seconds and uses client-chosen groups of ten with visible pauses. This is application policy, not a claim that Epic officially permits exactly ten requests. Recognized Epic throttling may continue automatically only through the guarded retry path. Unknown activation results are not blindly repeated, and the official Epic window remains a manual fallback without transferring input keys.

The three new review, wait and result screenshots use the actual 0.9.8 React renderer with fictional account, product and key fixtures. They are illustrative UI evidence, not real-account activations, a new release build, clean-Windows acceptance or measured GPU activity. Their source hashes and capture scope are retained in the screenshot capture receipt. The four original 0.9.0 images keep their original provenance and separate gallery label.

Version 0.9.9 fixes the `content_already_owned` batch-check response: products already present in the account are reported as already owned, checking continues through the remaining keys, and only ready rows enter the separately confirmed activation. Unknown results are not retried blindly. Installed and portable updates preserve the existing profile, sessions, tasks and user data; this patch does not require moving to a new profile or signing in again. These are source behavior descriptions, not a new real-account acceptance claim. The 0.9.8 batch screenshots and 0.9.0 gallery retain their actual capture versions.

Version 0.9.10 recognizes complete projects from the selected package manifest and
uses the corrected type for prepared downloads, saved tasks and completed Vault
packages. Matching saved packages are reused. A correction to Project clears any
previously selected existing-project destination and requires an explicit new
folder before preview or installation. The library feature copy explains this in
all nine site languages. Existing conflicting files remain blocked; optional
overwrite/replacement with confirmation and a backup is a future change, not a
0.9.10 feature. No new screenshots or real-account acceptance claims are added.

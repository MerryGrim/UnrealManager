# Unreal Manager website

Buildless static GitHub Pages site. Publish this directory with the repository's Pages workflow; no npm install or build is required.

- `index.html`: accessible page structure and English fallback copy.
- `translations.js`: all nine application languages; English interface strings are read from the HTML at startup.
- `app.js`: language and theme preferences, accessible System/Light/Dark segmented buttons, metadata, and shareable `?lang=ru&theme=light#features` links.
- `styles.css`: responsive dark and light themes, with no animation or external fonts.
- `assets/um-mark.svg`: original project mark.
- `assets/screenshots/*.webp`: genuine application UI with fictional demonstration data, English locale and the application's dark appearance. Screenshots do not verify real Epic/Fab transfers. See the repository's `THIRD_PARTY_NOTICES.md` for scope and attribution.

The website does not include analytics, external requests, a framework, or a package manager runtime. Language and theme are saved in browser local storage when available. The system theme follows the operating system's preference. All copy, image descriptions, control labels, and page metadata are translated.

Serve this directory using any local static HTTP server. For example, when Python is already available:

```powershell
python -m http.server 4173 --bind 127.0.0.1 --directory docs
```

Open `http://127.0.0.1:4173/`. Browser QA is covered by `verification/test-site.cjs` at repository root. Its requirements are development tools only.

The page links to the Windows x64 installer and portable ZIP for `v0.9.5`, alongside release notes and SHA-256 checksums. Source and build instructions remain available as secondary actions. Publish the matching GitHub release assets before deploying a website revision with new download links.

The page describes 0.9.5's sequential download-to-Vault queue, pending-item drag/keyboard priorities, explicit restart behavior, measured phase progress and optional Epic-registration warnings. Project copying remains a separate action. It also describes signed GitHub app updates for installed and portable copies, startup checks at most once daily, manual checks and a temporary apply helper. This website never performs an application update. Software-rendering configuration is distinguished from measured GPU activity. Demonstration screenshots are illustrative and do not establish clean-Windows compatibility or acceptance of every workflow.

`verification/capture-public-screenshots.cjs` uses the real frontend with a fictional account, self-authored covers, synthetic `X:/Fixture/Demo` paths and simulated queue progress. It blocks external requests and never starts the native app. Its `--preview` flag writes fresh private preview images instead of website assets. The app screenshot appearance remains dark; both website themes are independent of it.

Version 0.9.2 adds bounded parallel checks of large files while applying engine updates, with a separate current-file meter, old/new file checking stages and explicit replacement status. Actual application read bytes and read speed stay separate from completed logical output. Read metrics may include OS cache; they are not physical SSD benchmarks. No universal speed multiplier or ETA is claimed. Retained demonstration screenshots were captured with version 0.9.0 and illustrate the shared interface; they are not new runtime acceptance evidence for 0.9.2.

Version 0.9.3 checks rights before long fresh-install, update and resume work. A temporary headless UAC helper can handle only a mandatory Epic `.item` record; the original application and Epic session remain in the user profile. Canceling preserves the saved task. Protected engine folders require a writable location or corrected permissions, with read-only and sharing errors distinguished. The helper does not grant general engine write access or repair ACLs. Optional global Epic registration remains warning-only. These feature descriptions do not establish live UAC or clean-Windows acceptance for this version; the retained screenshots also do not establish 0.9.3 runtime acceptance.

Version 0.9.4 describes interrupted app-update recovery and completion acknowledgement after the new React interface initializes. Download recovery checks damaged partial blocks before fetching them again; checking, copying and rollback expose measured progress. A manually resumed queue task appears once while active, without deleting its persistent binding before success. Expired Epic authorization offers an explicit sign-in action while preserving tasks and the queue. Software-rendering arguments are supplied through all four WebView window-construction paths, including Epic sign-in and redemption. These are configuration claims, not measurements of zero GPU activity.

Version 0.9.5 adds seven local library sort orders and a page-size control below the results, records successfully created projects independently of their folder without a watcher, and suppresses blank-area context menus while preserving editable fields and custom menus. The explicit engine-registration check may request a temporary headless UAC broker only for Epic's fixed shared `LauncherInstalled.dat`; it does not elevate the application, engine-file writes or folder-permission changes. Date/size ordering is not included. New broker acceptance is recorded separately from previous release evidence.

The retained 0.9.0 demonstration screenshots remain unchanged. Local headless website checks cover nine languages, two themes, responsive layouts, keyboard controls and download links. Hosted Windows Server lifecycle results are distinct from clean Windows 10/11 desktop acceptance, standard-user UAC testing and physical GPU measurements; this page makes no 1.0-readiness or new human-UAC acceptance claim.

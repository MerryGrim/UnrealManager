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

The page links to the Windows x64 installer and portable ZIP for `v0.9.0`, alongside release notes and SHA-256 checksums. Source and build instructions remain available as secondary actions. Publish the matching GitHub release assets before deploying a website revision with new download links.

The page describes 0.9.0's sequential download-to-Vault queue, pending-item drag/keyboard priorities, explicit restart behavior, measured phase progress and optional Epic-registration warnings. Project copying remains a separate action. It also describes signed GitHub app updates for installed and portable copies, startup checks at most once daily, manual checks and a temporary apply helper. This website never performs an application update. Software-rendering configuration is distinguished from measured GPU activity. Demonstration screenshots are illustrative and do not establish clean-Windows compatibility or acceptance of every workflow.

`verification/capture-public-screenshots.cjs` uses the real frontend with a fictional account, self-authored covers, synthetic `X:/Fixture/Demo` paths and simulated queue progress. It blocks external requests and never starts the native app. Its `--preview` flag writes fresh private preview images instead of website assets. The app screenshot appearance remains dark; both website themes are independent of it.

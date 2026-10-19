# Unreal Manager website

Buildless static GitHub Pages site. Publish this directory with the repository's Pages workflow; no npm install or build is required.

- `index.html`: accessible page structure and English fallback copy.
- `translations.js`: all nine application languages; English interface strings are read from the HTML at startup.
- `app.js`: language and theme preferences, metadata, and shareable `?lang=ru&theme=light#features` links.
- `styles.css`: responsive dark and light themes, with no animation or external fonts.
- `assets/um-mark.svg`: original project mark.
- `assets/screenshots/*.webp`: genuine application UI with fictional demonstration data, English locale and the application's dark appearance. Screenshots do not verify real Epic/Fab transfers. See the repository's `THIRD_PARTY_NOTICES.md` for scope and attribution.

The website does not include analytics, external requests, a framework, or a package manager runtime. Language and theme are saved in browser local storage when available. The system theme follows the operating system's preference. All copy, image descriptions, control labels, and page metadata are translated.

Serve this directory using any local static HTTP server. For example, when Python is already available:

```powershell
python -m http.server 4173 --bind 127.0.0.1 --directory docs
```

Open `http://127.0.0.1:4173/`. Browser QA is covered by `verification/test-site.cjs` at repository root. Its requirements are development tools only.

Source and build links are the primary actions. The Releases link does not imply a prebuilt package has been published. Automatic application updates are described as planned and are not enabled by this site.

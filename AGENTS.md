# Unreal Manager requirements

- Keep the launcher lightweight. Development tools and test dependencies must not become runtime requirements.
- Preserve software rendering for all WebView2 windows, including Epic login and redemption. Set `--disable-gpu --disable-gpu-compositing` before creating windows; do not add WebGL, canvas animation, continuous rendering loops or decorative GPU effects.
- Distinguish configured software rendering from measured GPU activity. Verify processes when making claims about actual GPU use; do not infer hardware load merely from a Chromium `gpu-process`.
- Keep work files, caches and new tools on the workspace volume, using the configured tool root. Do not overwrite archived builds or verification evidence; new outputs belong in `Releases` and `verification/current`.
- Preserve account sessions, Vault data, download tasks and user projects. Mock UI regression checks do not establish that a real Epic/Fab transfer or full Unreal installation passed.

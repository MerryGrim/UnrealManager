// The builder API replaces Wry's generated WebView2 arguments. Retain the
// defaults from locked Wry 0.55.1, including its enabled-by-default autoplay,
// before adding our software-rendering policy. The environment fallback alone
// is insufficient for elevated hosts that ignore WebView2 environment overrides.
pub(crate) const BROWSER_ARGUMENTS: &str = concat!(
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
    " --autoplay-policy=no-user-gesture-required",
    " --disable-gpu --disable-gpu-compositing",
);

#[cfg(test)]
#[path = "webview_policy_tests.rs"]
mod tests;

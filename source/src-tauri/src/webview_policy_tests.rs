use super::BROWSER_ARGUMENTS;

#[test]
fn policy_retains_the_locked_wry_webview2_defaults() {
    // An SDK upgrade requires reviewing its default argument generation rather
    // than silently replacing new defaults with this pinned policy.
    let package = include_str!("../Cargo.lock")
        .split("[[package]]")
        .find(|entry| entry.lines().any(|line| line == "name = \"wry\""))
        .expect("Locked Wry package");
    assert!(package.lines().any(|line| line == "version = \"0.55.1\""));

    let flags: Vec<_> = BROWSER_ARGUMENTS.split_whitespace().collect();
    let features: Vec<_> = flags.iter()
        .filter_map(|flag| flag.strip_prefix("--disable-features="))
        .collect();
    assert_eq!(features.len(), 1, "Do not replace a preceding Chromium feature switch");
    let disabled: Vec<_> = features[0].split(',').collect();
    for feature in ["msWebOOUI", "msPdfOOUI", "msSmartScreenProtection"] {
        assert!(disabled.contains(&feature), "Keep locked Wry default: {feature}");
    }
    assert!(flags.contains(&"--autoplay-policy=no-user-gesture-required"),
        "Keep Wry's enabled-by-default autoplay behavior");
}

#[test]
fn software_policy_has_both_switches_without_security_or_debug_overrides() {
    let flags: Vec<_> = BROWSER_ARGUMENTS.split_whitespace().collect();
    for required in ["--disable-gpu", "--disable-gpu-compositing"] {
        assert_eq!(flags.iter().filter(|flag| **flag == required).count(), 1);
    }
    for forbidden in ["--enable-gpu", "--disable-web-security", "--no-sandbox",
        "--disable-gpu-sandbox", "--remote-debugging", "--proxy-server"] {
        assert!(!flags.iter().any(|flag| flag.starts_with(forbidden)),
            "Rendering policy must not introduce unrelated overrides: {forbidden}");
    }
}

#[test]
fn all_current_webview_constructors_set_the_shared_api_policy_before_build() {
    // Static constructor coverage, not native-window or measured-GPU proof.
    // Ordinary compilation also type-checks the actual Tauri builder calls.
    let sources = [
        ("main from_config", include_str!("lib.rs")),
        ("Epic login", include_str!("epic_login.rs")),
        ("Epic redemption", include_str!("epic_redeem.rs")),
        ("hidden Epic batch redemption", include_str!("epic_redeem_batch.rs")),
        ("hidden Epic session reset", include_str!("epic_browser.rs")),
    ];
    let mut constructors = 0;
    for (name, source) in sources {
        let mut found = 0;
        for rest in source.split("WebviewWindowBuilder::").skip(1) {
            assert!(rest.starts_with("new(") || rest.starts_with("from_config("),
                "Review an unfamiliar WebView constructor: {name}");
            let chain = rest.split_once(".build(").expect("Builder reaches build").0;
            assert!(chain.contains(".additional_browser_args(crate::webview_policy::BROWSER_ARGUMENTS)"),
                "Missing programmatic software policy before build: {name}");
            assert_eq!(chain.matches(".additional_browser_args(").count(), 1,
                "A later argument setter could replace the policy: {name}");
            found += 1;
            constructors += 1;
        }
        assert_eq!(found, 1, "Review changes to the current constructor inventory: {name}");
    }
    assert_eq!(constructors, 5);
}

#[test]
fn environment_fallback_remains_before_starting_the_application() {
    let main = include_str!("main.rs");
    let setup = main.find("\"WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS\"")
        .expect("Existing environment fallback retained");
    let run = main.find("unreal_manager_lib::run();").expect("Normal application entry");
    assert!(setup < run);
    assert!(main[setup..run].contains("\"--disable-gpu --disable-gpu-compositing\""));
}

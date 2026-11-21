//! Release-channel evidence from Epic identifiers and catalog descriptors.
//! A zero patch, Live label or +Release branch alone do not establish stability.
use egs_api::api::types::epic_asset::EpicAsset;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_RECORDS: usize = 1024;
const MAX_AGE: u64 = 24 * 60 * 60;
const EVIDENCE_VERSION: u32 = 2;

// Public app/build projection of the 38 Windows Live catalog entries observed
// on 2026-11-21. No account identifiers, installation paths or credentials.
#[cfg(test)]
pub(crate) const OBSERVED_WINDOWS_LIVE_BUILDS: [(&str, &str); 38] = [
    ("UE_4.0", "4.0.2-2034640+++depot+UE4-Releases+4.0-Windows"),
    ("UE_4.1", "4.1.2-2109544+++depot+UE4-Releases+4.1-Windows"),
    ("UE_4.2", "4.2.1-2101290+++depot+UE4-Releases+4.2-Windows"),
    (
        "UE_4.3",
        "4.3.1-2226335+++depot+UE4-Releases+4.3-2023.1-Windows",
    ),
    (
        "UE_4.4",
        "4.4.3-2296771+++depot+UE4-Releases+4.4-2023.1-Windows",
    ),
    (
        "UE_4.5",
        "4.5.1-2336687+++depot+UE4-Releases+4.5-2023.1-Windows",
    ),
    (
        "UE_4.6",
        "4.6.1-2386410+++depot+UE4-Releases+4.6-2023.1-Windows",
    ),
    (
        "UE_4.7",
        "4.7.6-2513093+++depot+UE4-Releases+4.7-2023.1-Windows",
    ),
    (
        "UE_4.8",
        "4.8.3-2634408+++depot+UE4-Releases+4.8-2023.1-Windows",
    ),
    (
        "UE_4.9",
        "4.9.2-2707645+++depot+UE4-Releases+4.9-2023.1-Windows",
    ),
    (
        "UE_4.10",
        "4.10.4-2872498+++depot+UE4-Releases+4.10-2023.1-Windows",
    ),
    (
        "UE_4.11",
        "4.11.2-2946394+++UE4+Release-4.11-2023.1-Windows",
    ),
    (
        "UE_4.12",
        "4.12.5-3039270+++UE4+Release-4.12-2023.1-Windows",
    ),
    (
        "UE_4.13",
        "4.13.2-3172292+++UE4+Release-4.13-2023.1-Windows",
    ),
    (
        "UE_4.14",
        "4.14.3-3249277+++UE4+Release-4.14-2023.1-Windows",
    ),
    (
        "UE_4.15",
        "4.15.3-3450819+++UE4+Release-4.15-2023.1-Windows",
    ),
    (
        "UE_4.16",
        "4.16.3-3561208+++UE4+Release-4.16-2023.1-Windows",
    ),
    (
        "UE_4.17",
        "4.17.2-3658906+++UE4+Release-4.17-2023.1-Windows",
    ),
    (
        "UE_4.18",
        "4.18.3-3832480+++UE4+Release-4.18-2023.1-Windows",
    ),
    (
        "UE_4.19",
        "4.19.2-4033788+++UE4+Release-4.19-2023.1-Windows",
    ),
    (
        "UE_4.20",
        "4.20.3-4369336+++UE4+Release-4.20-2023.1-Windows",
    ),
    (
        "UE_4.21",
        "4.21.2-4753647+++UE4+Release-4.21-2023.1-Windows",
    ),
    (
        "UE_4.22",
        "4.22.3-7053642+++UE4+Release-4.22-2023.1-Windows",
    ),
    (
        "UE_4.23",
        "4.23.1-9631420+++UE4+Release-4.23-2023.1-Windows",
    ),
    (
        "UE_4.24",
        "4.24.3-11590370+++UE4+Release-4.24-2023.1a-Windows",
    ),
    (
        "UE_4.25",
        "4.25.4-14469661+++UE4+Release-4.25-2023.1a-Windows",
    ),
    (
        "UE_4.26",
        "4.26.2-15973114+++UE4+Release-4.26-2023.1a-Windows",
    ),
    (
        "UE_4.27",
        "4.27.2-18319896+++UE4+Release-4.27-2023.1a-Windows",
    ),
    ("UE_5.0", "5.0.3-20979098+++UE5+Release-5.0-2023.1a-Windows"),
    ("UE_5.1", "5.1.1-23901901+++UE5+Release-5.1-2023.1a-Windows"),
    ("UE_5.2", "5.2.1-26001984+++UE5+Release-5.2a-Windows"),
    ("UE_5.3", "5.3.2-29314046+++UE5+Release-5.3a-Windows"),
    ("UE_5.4", "5.4.4-35576357+++UE5+Release-5.4a-Windows"),
    ("UE_5.5", "5.5.4-40574608+++UE5+Release-5.5-Windows"),
    ("UE_5.6", "5.6.1-44394996+++UE5+Release-5.6-Windows"),
    ("UE_5.7", "5.7.4-51494982+++UE5+Release-5.7-Windows"),
    ("UE_5.8", "5.8.4-58902052+++UE5+Release-5.8-Windows"),
    ("UE_5.9", "5.9.0-60738873+++UE5+Release-5.9-Windows"),
];

/// Some official Launcher builds omit all channel markers, including Preview 2.
/// Match a published version AND changelist, never an entire numbered branch:
/// a later 5.9 final build must not inherit an earlier preview's channel.
fn published_build(app: &str, build: &str) -> Option<&'static str> {
    let (version, rest) = build.split_once('-')?;
    let changelist = rest.split('+').next()?.parse::<u64>().ok()?;
    match (app, version, changelist) {
        // Epic Preview 2 build info, 2026-11-19 (CL and GitHub preview tag):
        // https://forums.unrealengine.com/t/preview-2-unreal-engine-5-9-preview/2852840/42
        ("UE_5.9", "5.9.0", 60_738_873) => Some("preview"),
        // Epic's released hotfix plus the exact public crash-report build:
        // https://forums.unrealengine.com/t/5-7-4-hotfix-released/2705041
        // https://issues.unrealengine.com/issue/UE-282900
        ("UE_5.7", "5.7.4", 51_494_982) => Some("stable"),
        // Epic's released 5.8.4 hotfix; exact Windows Launcher Build.version:
        // https://forums.unrealengine.com/t/ue-5-8-4-launcher-install-c-build-fails-with-ruleserror-fabtoolset-after-fab-plugin-0-0-22-is-installed/2846815
        // https://forums.unrealengine.com/t/5-8-4-hotfix-released/2842775
        ("UE_5.8", "5.8.4", 58_902_052) => Some("stable"),
        _ => None,
    }
}

/// Epic identifies a positive third version component as a HotFix, rather than
/// an initial/preview release. Bound this fallback to already released branches
/// and the matching Windows Live Launcher identity; neither an arbitrary numeric
/// version nor a +Release branch by itself is positive release evidence.
/// https://dev.epicgames.com/documentation/en-us/unreal-engine/install-unreal-engine#applyhotfixupdates
/// https://www.unrealengine.com/blog/unreal-engine-4-27-released
/// https://www.unrealengine.com/news/unreal-engine-5-8-is-now-available
fn historical_windows_hotfix(app: &str, build: &str, label: &str) -> bool {
    // Empty labels occur only when reading the public Windows Live catalog cache,
    // whose previous schema omitted the label returned by Epic's asset endpoint.
    if !matches!(label, "Live" | "") {
        return false;
    }
    let Some((version, rest)) = build.split_once('-') else {
        return false;
    };
    let Ok(version) = semver::Version::parse(version) else {
        return false;
    };
    if version.patch == 0
        || !version.pre.is_empty()
        || !version.build.is_empty()
        || !matches!((version.major, version.minor), (4, 0..=27) | (5, 0..=8))
        || app != format!("UE_{}.{}", version.major, version.minor)
    {
        return false;
    }
    let Some((changelist, branch)) = rest.split_once("+++") else {
        return false;
    };
    if changelist.is_empty()
        || !changelist.bytes().all(|c| c.is_ascii_digit())
        || !changelist.parse::<u64>().is_ok_and(|cl| cl > 0)
    {
        return false;
    }
    let Some(branch) = branch.strip_suffix("-Windows") else {
        return false;
    };
    // These optional suffixes are Epic's redistributed historical Launcher builds.
    let branch = branch
        .strip_suffix("-2023.1a")
        .or_else(|| branch.strip_suffix("-2023.1"))
        .unwrap_or(branch);
    let release = format!(
        "UE{}+Release-{}.{}",
        version.major, version.major, version.minor
    );
    branch == release
        || branch == format!("{release}a")
        || (version.major == 4 && branch == format!("depot+UE4-Releases+4.{}", version.minor))
}

fn identifier_channel(app: &str, build: &str, label: &str) -> Option<&'static str> {
    if [app, build, label].into_iter().any(preview) {
        Some("preview")
    } else {
        published_build(app, build)
    }
}

/// Historical HotFix evidence survives an unavailable catalog request, but stays
/// provisional until metadata is fetched. In particular it must not suppress a
/// cached or fresh explicit Preview marker from Epic.
pub(crate) fn initial_channel(asset: &EpicAsset, cached: Option<&str>) -> (&'static str, bool) {
    let identifier = identifier_channel(&asset.app_name, &asset.build_version, &asset.label_name);
    if identifier == Some("preview") || cached == Some("preview") {
        return ("preview", false);
    }
    if let Some(channel) = identifier {
        return (channel, false);
    }
    if cached == Some("stable") {
        return ("stable", false);
    }
    let provisional = classify(
        &asset.app_name,
        &asset.build_version,
        &asset.label_name,
        None,
    );
    (provisional, cached.is_none())
}

fn words(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
fn preview(value: &str) -> bool {
    let words = words(value);
    words.iter().any(|w| {
        matches!(
            w.as_str(),
            "preview"
                | "prerelease"
                | "alpha"
                | "beta"
                | "rc"
                | "earlyaccess"
                | "ea"
                | "nightly"
                | "experimental"
        )
    }) || words.windows(2).any(|pair| {
        matches!(
            (pair[0].as_str(), pair[1].as_str()),
            ("early", "access") | ("pre", "release") | ("release", "candidate")
        )
    })
}
fn stable_descriptor(value: &str) -> bool {
    words(value)
        .iter()
        .any(|w| matches!(w.as_str(), "stable" | "production" | "final"))
}
fn semantic_key(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Unknown evidence remains unknown; callers can display it without asserting a
/// verified production release. Preview evidence always wins over a stable marker.
pub(crate) fn classify(
    app: &str,
    build: &str,
    label: &str,
    metadata: Option<&Value>,
) -> &'static str {
    let published = identifier_channel(app, build, label);
    if published == Some("preview") {
        return "preview";
    }
    let fallback =
        published.or_else(|| historical_windows_hotfix(app, build, label).then_some("stable"));
    let Some(metadata) = metadata else {
        return fallback.unwrap_or("unknown");
    };
    let mut descriptors = Vec::new();
    if let Some(title) = metadata["title"].as_str() {
        descriptors.push(title);
    }
    for release in metadata["releaseInfo"].as_array().into_iter().flatten() {
        if release["appId"].as_str() == Some(app) {
            if let Some(title) = release["versionTitle"].as_str() {
                descriptors.push(title);
            }
        }
    }
    let mut explicit_stable = false;
    if let Some(attributes) = metadata["customAttributes"].as_object() {
        // Interpret only attributes whose names explicitly describe a release
        // channel. Unknown catalog attributes and descriptive text are ignored.
        for (key, attribute) in attributes {
            let key = semantic_key(key);
            let value = attribute.get("value").unwrap_or(attribute);
            if matches!(key.as_str(), "ispreview" | "isprerelease") {
                let boolean = value.as_bool().or_else(|| {
                    value
                        .as_str()
                        .and_then(|v| match v.to_ascii_lowercase().as_str() {
                            "true" | "1" => Some(true),
                            "false" | "0" => Some(false),
                            _ => None,
                        })
                });
                if boolean == Some(true) {
                    return "preview";
                }
                explicit_stable |= boolean == Some(false);
            } else if matches!(key.as_str(), "releasechannel" | "buildchannel") {
                if let Some(value) = value.as_str() {
                    if preview(value) {
                        return "preview";
                    }
                    explicit_stable |=
                        stable_descriptor(value) || value.eq_ignore_ascii_case("release");
                }
            }
        }
    }
    if descriptors.iter().any(|value| preview(value)) {
        return "preview";
    }
    if explicit_stable
        || descriptors.iter().any(|value| stable_descriptor(value))
        || fallback == Some("stable")
    {
        return "stable";
    }
    "unknown"
}

pub(crate) fn fingerprint(asset: &EpicAsset) -> String {
    let identity = serde_json::json!([
        asset.namespace,
        asset.catalog_item_id,
        asset.app_name,
        asset.build_version,
        asset.label_name
    ]);
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Cache {
    records: BTreeMap<String, Record>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    channel: String,
    checked_at: u64,
    #[serde(default)]
    evidence_version: u32,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl Cache {
    pub(crate) fn read(path: &Path) -> Self {
        let read = || -> Option<Self> {
            crate::vault::no_links(path).ok()?;
            let file = fs::File::open(path).ok()?;
            if file.metadata().ok()?.len() > MAX_CACHE_BYTES {
                return None;
            }
            let mut bytes = Vec::new();
            file.take(MAX_CACHE_BYTES + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > MAX_CACHE_BYTES {
                return None;
            }
            let cache: Self = serde_json::from_slice(&bytes).ok()?;
            if cache.records.len() > MAX_RECORDS
                || cache.records.iter().any(|(key, record)| {
                    key.len() != 64
                        || !key.bytes().all(|b| b.is_ascii_hexdigit())
                        || !matches!(record.channel.as_str(), "stable" | "preview" | "unknown")
                })
            {
                return None;
            }
            Some(cache)
        };
        read().unwrap_or_default()
    }
    pub(crate) fn get(&self, asset: &EpicAsset) -> Option<&str> {
        self.records
            .get(&fingerprint(asset))
            .filter(|r| {
                let current = now();
                r.evidence_version == EVIDENCE_VERSION
                    && r.checked_at <= current
                    && current - r.checked_at < MAX_AGE
            })
            .map(|r| r.channel.as_str())
    }
    pub(crate) fn insert(&mut self, asset: &EpicAsset, channel: &str) {
        if !matches!(channel, "stable" | "preview" | "unknown") {
            return;
        }
        self.records.insert(
            fingerprint(asset),
            Record {
                channel: channel.into(),
                checked_at: now(),
                evidence_version: EVIDENCE_VERSION,
            },
        );
        while self.records.len() > MAX_RECORDS {
            let Some(key) = self
                .records
                .iter()
                .min_by_key(|(_, record)| record.checked_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.records.remove(&key);
        }
    }
    pub(crate) fn save(&self, path: &Path) {
        if crate::vault::no_links(path).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(self) else {
            return;
        };
        if bytes.len() as u64 <= MAX_CACHE_BYTES {
            let _ = crate::storage::atomic(path, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_windows_live_catalog_keeps_all_37_hotfixes_and_one_preview() {
        let mut counts = BTreeMap::new();
        for (app, build) in OBSERVED_WINDOWS_LIVE_BUILDS {
            let expected = if app == "UE_5.9" { "preview" } else { "stable" };
            assert_eq!(
                classify(app, build, "Live", None),
                expected,
                "{app}: {build}"
            );
            assert_eq!(
                classify(app, build, "", None),
                expected,
                "offline {app}: {build}"
            );
            *counts.entry(expected).or_insert(0) += 1;
        }
        assert_eq!(counts, BTreeMap::from([("preview", 1), ("stable", 37)]));
    }
    #[test]
    fn historical_hotfix_fallback_requires_the_entire_bounded_launcher_identity() {
        for (app, build, label) in [
            ("UE_5.6", "5.6.0-44394996+++UE5+Release-5.6-Windows", "Live"),
            ("UE_5.6", "5.6.1-0+++UE5+Release-5.6-Windows", "Live"),
            ("UE_5.6", "5.6.1-x+++UE5+Release-5.6-Windows", "Live"),
            (
                "UE_5.6",
                "5.6.1-18446744073709551616+++UE5+Release-5.6-Windows",
                "Live",
            ),
            ("UE_5.6", "5.6.1-44394996+++UE5+Release-5.5-Windows", "Live"),
            ("UE_5.6", "5.6.1-44394996+++UE4+Release-5.6-Windows", "Live"),
            ("UE_5.5", "5.6.1-44394996+++UE5+Release-5.6-Windows", "Live"),
            ("UE_5.6", "5.6.1-44394996+++UE5+Release-5.6-Mac", "Live"),
            ("UE_5.6", "5.6.1-44394996+++UE5+Release-5.6-Windows", "Dev"),
            (
                "UE_5.6",
                "5.6.1-44394996+++UE5+Release-5.6-custom-Windows",
                "Live",
            ),
            (
                "UE_5.6",
                "5.6.1-44394996+++UE5+Release-5.6-2024.2-Windows",
                "Live",
            ),
            (
                "UE_5.6",
                "5.6.1-44394996+++UE5+Release-5.6b-Windows",
                "Live",
            ),
            ("UE_5.6", "5.6.1-44394996+++UE5+Release-5.6", "Live"),
            ("UE_5.6", "5.6.1-44394996", "Live"),
            ("UE_5.9", "5.9.1-70000000+++UE5+Release-5.9-Windows", "Live"),
            (
                "UE_5.10",
                "5.10.1-70000000+++UE5+Release-5.10-Windows",
                "Live",
            ),
            ("UE_6.0", "6.0.1-70000000+++UE6+Release-6.0-Windows", "Live"),
            (
                "UE_4.28",
                "4.28.1-70000000+++UE4+Release-4.28-Windows",
                "Live",
            ),
        ] {
            assert_eq!(
                classify(app, build, label, None),
                "unknown",
                "{app}: {build} ({label})"
            );
        }
    }
    #[test]
    fn provisional_hotfix_still_fetches_metadata_and_preview_evidence_always_wins() {
        let asset = EpicAsset {
            app_name: "UE_5.6".into(),
            build_version: "5.6.1-44394996+++UE5+Release-5.6-Windows".into(),
            label_name: "Live".into(),
            ..Default::default()
        };
        assert_eq!(initial_channel(&asset, None), ("stable", true));
        assert_eq!(initial_channel(&asset, Some("unknown")), ("stable", false));
        assert_eq!(initial_channel(&asset, Some("preview")), ("preview", false));
        // If the request fails, the provisional stable result remains usable;
        // successful positive Preview metadata overrides it before cache save.
        let metadata = serde_json::json!({"customAttributes":{"IsPreview":{"value":"true"}}});
        assert_eq!(
            classify(
                &asset.app_name,
                &asset.build_version,
                &asset.label_name,
                Some(&metadata)
            ),
            "preview"
        );
        assert_eq!(
            classify(&asset.app_name, &asset.build_version, "Preview", None),
            "preview"
        );
        assert_eq!(
            classify(
                &asset.app_name,
                "5.6.1-preview-44394996+++UE5+Release-5.6-Windows",
                "Live",
                None
            ),
            "preview"
        );
    }
    #[test]
    fn official_exact_build_evidence_does_not_classify_entire_branches() {
        for (app, build, expected) in [
            (
                "UE_5.9",
                "5.9.0-60738873+++UE5+Release-5.9-Windows",
                "preview",
            ),
            (
                "UE_5.8",
                "5.8.4-58902052+++UE5+Release-5.8-Windows",
                "stable",
            ),
            (
                "UE_5.7",
                "5.7.4-51494982+++UE5+Release-5.7-Windows",
                "stable",
            ),
            (
                "UE_5.9",
                "5.9.0-60738874+++UE5+Release-5.9-Windows",
                "unknown",
            ),
            (
                "UE_5.9",
                "5.9.1-60738873+++UE5+Release-5.9-Windows",
                "unknown",
            ),
            (
                "UE_5.8",
                "5.9.0-60738873+++UE5+Release-5.9-Windows",
                "unknown",
            ),
            (
                "UE_5.8",
                "5.8.0-58902053+++UE5+Release-5.8-Windows",
                "unknown",
            ),
        ] {
            assert_eq!(
                classify(app, build, "Live", None),
                expected,
                "{app}: {build}"
            );
        }
        let stable = serde_json::json!({"customAttributes":{"ReleaseChannel":{"value":"Stable"}}});
        assert_eq!(
            classify(
                "UE_5.9",
                "5.9.0-60738873+++UE5+Release-5.9",
                "Live",
                Some(&stable)
            ),
            "preview"
        );
        let preview = serde_json::json!({"title":"Unreal Engine Preview"});
        assert_eq!(
            classify(
                "UE_5.8",
                "5.8.4-58902052+++UE5+Release-5.8",
                "Live",
                Some(&preview)
            ),
            "preview"
        );
    }
    #[test]
    fn explicit_identifiers_detect_preview_and_early_access() {
        for (app, build, label) in [
            ("UE_5.9", "5.9.0-preview2-123", "Live"),
            ("UE_5.0EA", "5.0.0-123", "Live"),
            ("UE_5.9", "5.9.0-123", "Preview"),
            ("UE_5.9_Preview1", "5.9.0-123", "Live"),
        ] {
            assert_eq!(classify(app, build, label, None), "preview");
        }
        assert_eq!(
            classify("UE_5.9", "5.9.0-123+++UE5+Release-5.9", "Live", None),
            "unknown"
        );
    }
    #[test]
    fn catalog_descriptors_resolve_numeric_preview_without_description_false_positives() {
        let metadata = serde_json::json!({"title":"Unreal Engine 5.9 Preview", "description":"Production ready soon"});
        assert_eq!(
            classify("UE_5.9", "5.9.0-123", "Live", Some(&metadata)),
            "preview"
        );
        let metadata = serde_json::json!({"title":"Unreal Engine", "description":"Earlier preview builds introduced this feature", "releaseInfo":[{"appId":"UE_5.8", "versionTitle":"5.8 Preview"}]});
        assert_eq!(
            classify("UE_5.9", "5.9.0-123", "Live", Some(&metadata)),
            "unknown"
        );
        let metadata = serde_json::json!({"releaseInfo":[{"appId":"UE_5.9", "versionTitle":"5.9 Early Access"}]});
        assert_eq!(
            classify("UE_5.9", "5.9.0-123", "Live", Some(&metadata)),
            "preview"
        );
    }
    #[test]
    fn explicit_channel_attributes_and_stable_markers_preserve_preview_precedence() {
        for metadata in [
            serde_json::json!({"customAttributes":{"IsPrerelease":{"value":"true"}}}),
            serde_json::json!({"customAttributes":{"ReleaseChannel":{"value":"Preview"}}}),
        ] {
            assert_eq!(
                classify("UE_5.9", "5.9.0-123", "Live", Some(&metadata)),
                "preview"
            );
        }
        let metadata = serde_json::json!({"customAttributes":{"IsPreview":{"value":"false"}}});
        assert_eq!(
            classify("UE_5.8", "5.8.0-123", "Live", Some(&metadata)),
            "stable"
        );
        let metadata = serde_json::json!({"title":"Unreal Engine Preview", "customAttributes":{"BuildChannel":{"value":"Stable"}}});
        assert_eq!(
            classify("UE_5.9", "5.9.0-123", "Live", Some(&metadata)),
            "preview"
        );
        let metadata = serde_json::json!({"title":"Unreal Engine production release"});
        assert_eq!(
            classify("UE_5.8", "5.8.0-123", "Live", Some(&metadata)),
            "stable"
        );
    }
    #[test]
    fn cache_fingerprint_changes_with_build_and_label() {
        let mut asset = EpicAsset {
            app_name: "UE_5.9".into(),
            build_version: "5.9.0-1".into(),
            label_name: "Live".into(),
            namespace: "ue".into(),
            catalog_item_id: "fixture".into(),
            ..Default::default()
        };
        let mut cache = Cache::default();
        cache.insert(&asset, "preview");
        assert_eq!(cache.get(&asset), Some("preview"));
        asset.build_version = "5.9.0-2".into();
        assert_eq!(cache.get(&asset), None);
        asset.build_version = "5.9.0-1".into();
        asset.label_name = "Other".into();
        assert_eq!(cache.get(&asset), None);
    }
    #[test]
    fn stale_channel_records_and_future_timestamps_are_not_reused() {
        let asset = EpicAsset {
            app_name: "UE_5.9".into(),
            build_version: "5.9.0-1".into(),
            ..Default::default()
        };
        let key = fingerprint(&asset);
        let mut cache: Cache = serde_json::from_value(
            serde_json::json!({"records":{(key.clone()):{"channel":"stable","checked_at":now()}}}),
        )
        .unwrap();
        assert_eq!(cache.get(&asset), None);
        cache.insert(&asset, "preview");
        assert_eq!(cache.get(&asset), Some("preview"));
        cache.records.get_mut(&key).unwrap().evidence_version = EVIDENCE_VERSION - 1;
        assert_eq!(cache.get(&asset), None);
        cache.records.get_mut(&key).unwrap().evidence_version = EVIDENCE_VERSION;
        cache.records.get_mut(&key).unwrap().checked_at = now() + MAX_AGE;
        assert_eq!(cache.get(&asset), None);
        cache.records.get_mut(&key).unwrap().checked_at = now() - MAX_AGE;
        assert_eq!(cache.get(&asset), None);
    }
    #[test]
    fn complete_asset_keys_keep_same_branch_preview_and_final_separate() {
        let preview = EpicAsset {
            app_name: "UE_5.9".into(),
            build_version: "5.9.0-60738873+++UE5+Release-5.9-Windows".into(),
            namespace: "ue".into(),
            catalog_item_id: "fixture".into(),
            label_name: "Live".into(),
            ..Default::default()
        };
        let final_build = EpicAsset {
            build_version: "5.9.0-70000000+++UE5+Release-5.9-Windows".into(),
            ..preview.clone()
        };
        let channels = BTreeMap::from([
            (fingerprint(&preview), "preview"),
            (fingerprint(&final_build), "stable"),
        ]);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[&fingerprint(&preview)], "preview");
        assert_eq!(channels[&fingerprint(&final_build)], "stable");
    }
}

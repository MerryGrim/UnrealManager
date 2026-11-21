//! Release-channel evidence from Epic identifiers and catalog descriptors.
//! A zero patch, Live label and +Release branch do not establish stability.
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
const EVIDENCE_VERSION: u32 = 1;

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
    if [app, build, label].into_iter().any(preview) {
        return "preview";
    }
    let published = published_build(app, build);
    if published == Some("preview") {
        return "preview";
    }
    let Some(metadata) = metadata else {
        return published.unwrap_or("unknown");
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
        || published == Some("stable")
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
                "5.8.4-58902053+++UE5+Release-5.8-Windows",
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

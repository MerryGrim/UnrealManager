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
    let Some(metadata) = metadata else {
        return "unknown";
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
    if explicit_stable || descriptors.iter().any(|value| stable_descriptor(value)) {
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
            .filter(|r| now().saturating_sub(r.checked_at) < MAX_AGE)
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
}

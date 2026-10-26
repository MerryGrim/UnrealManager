//! Explicit-sync, metadata-only classification. No manifests or background work.
use crate::epic::LibraryItem;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet, VecDeque}, fs, io::Read, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};
mod legacy;

const SCHEMA: u8 = 1;
const MAX_RECORDS: usize = 4096;
const MAX_CACHE: usize = 4 * 1024 * 1024;
const MAX_RESPONSE: usize = 512 * 1024;
const MAX_TRANSFER: usize = 128 * 1024 * 1024;
const MAX_AGE: u64 = 30 * 24 * 60 * 60;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const SYNC_TIMEOUT: Duration = Duration::from_secs(6 * 60);
const FAB: &str = "https://www.fab.com";

#[derive(Default, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Stats {
    pub total: usize,
    pub done: usize,
    pub unresolved: usize,
    pub fetched: usize,
    pub cached: usize,
    pub legacy_fetched: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record { listing: String, fingerprint: String, kind: String, checked_at: u64 }
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cache { schema: u8, account_hash: String, records: BTreeMap<String, Record> }
#[derive(Clone)]
struct Candidate { index: usize, id: String, listing: String, fingerprint: String, legacy: Option<LegacyAsset> }
#[derive(Clone)]
struct LegacyAsset { namespace: String, artifact: String }

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
fn account_key(account: &str) -> Option<String> {
    (account.len() == 32 && account.bytes().all(|b| b.is_ascii_hexdigit())).then(|| account.to_ascii_lowercase())
}
fn account_hash(account: &str) -> String { format!("{:x}", Sha256::digest(format!("fab-types-v1:{account}"))) }
fn empty_cache(account: &str) -> Cache { Cache { schema: SCHEMA, account_hash: account_hash(account), records: BTreeMap::new() } }
fn cache_path(base: &Path, account: &str) -> Option<PathBuf> { Some(base.join(format!("{}-types.json", account_key(account)?))) }
fn valid_kind(kind: &str) -> bool { matches!(kind, "Asset" | "Project" | "Plugin") }
fn valid_record(id: &str, record: &Record) -> bool {
    !id.is_empty() && id.len() <= 256 && valid_kind(&record.kind)
        && uuid::Uuid::parse_str(&record.listing).is_ok()
        && record.fingerprint.len() == 64 && record.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
}
fn decode_cache(bytes: &[u8], account: &str) -> Option<Cache> {
    if bytes.len() > MAX_CACHE { return None; }
    let cache: Cache = serde_json::from_slice(bytes).ok()?;
    if cache.schema != SCHEMA || cache.account_hash != account_hash(account) || cache.records.len() > MAX_RECORDS
        || cache.records.iter().any(|(id, record)| !valid_record(id, record)) { return None; }
    Some(cache)
}
fn read_cache(base: &Path, account: &str) -> Cache {
    let read = || -> Option<Cache> {
        let path = cache_path(base, account)?;
        crate::vault::no_links(&path).ok()?;
        let file = fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_CACHE as u64 { return None; }
        let mut bytes = Vec::new();
        file.take(MAX_CACHE as u64 + 1).read_to_end(&mut bytes).ok()?;
        decode_cache(&bytes, account)
    };
    read().unwrap_or_else(|| empty_cache(account))
}
fn save_cache(base: &Path, account: &str, cache: &Cache) {
    let Some(path) = cache_path(base, account) else { return; };
    if cache.records.len() > MAX_RECORDS || crate::vault::no_links(&path).is_err() { return; }
    let Ok(bytes) = serde_json::to_vec(cache) else { return; };
    if bytes.len() > MAX_CACHE { return; }
    // Derived public metadata only. Failure must not discard a synchronized library.
    let _ = crate::storage::atomic(&path, &bytes);
}

fn listing_uid(value: &str) -> Option<String> {
    if value.len() > 2048 { return None; }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("fab.com" | "www.fab.com"))
        || url.port_or_known_default() != Some(443) || !url.username().is_empty() || url.password().is_some() { return None; }
    let parts: Vec<_> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
    let id = match parts.as_slice() {
        ["listings", id] => *id,
        [locale, "listings", id] if locale.len() <= 16 && locale.split('-').enumerate().all(|(index,part)| part.bytes().all(|b|b.is_ascii_alphabetic()) && match index {0=>matches!(part.len(),2|3),1=>matches!(part.len(),2|4),2=>part.len()==2,_=>false}) => *id,
        _ => return None,
    };
    Some(uuid::Uuid::parse_str(id).ok()?.to_string())
}
fn fingerprint(item: &LibraryItem) -> Option<String> {
    if item.versions.len() > 1024 { return None; }
    let mut versions = Vec::new();
    for version in &item.versions {
        if version.artifact_id.len() > 256 || version.engines.len() > 128 || version.platforms.len() > 128
            || version.engines.iter().chain(&version.platforms).any(|v| v.len() > 128) { return None; }
        let mut engines = version.engines.clone(); engines.sort(); engines.dedup();
        let mut platforms = version.platforms.clone(); platforms.sort(); platforms.dedup();
        versions.push((version.artifact_id.clone(), engines, platforms));
    }
    versions.sort(); versions.dedup();
    Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&versions).ok()?)))
}
fn candidate(index: usize, item: &LibraryItem) -> Option<Candidate> {
    if item.id.is_empty() || item.id.len() > 256 { return None; }
    Some(Candidate { index, id: item.id.clone(), listing: listing_uid(&item.listing_url)?, fingerprint: fingerprint(item)?, legacy: legacy_asset(item) })
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && value.len() <= 256 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}
fn engine_key(value: &str) -> (u32, u32, u32) {
    let mut numbers = value.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse::<u32>().ok());
    (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0), numbers.next().unwrap_or(0))
}
fn legacy_asset(item: &LibraryItem) -> Option<LegacyAsset> {
    if !identifier(&item.namespace) || !identifier(&item.id) { return None; }
    let version = item.versions.iter().filter(|version| identifier(&version.artifact_id)
        && version.platforms.iter().any(|platform| platform.eq_ignore_ascii_case("Windows")))
        .max_by_key(|version| (version.engines.iter().map(|engine| engine_key(engine)).max().unwrap_or_default(), version.artifact_id.as_str()))?;
    Some(LegacyAsset { namespace: item.namespace.clone(), artifact: version.artifact_id.clone() })
}
fn matching<'a>(cache: &'a Cache, candidate: &Candidate) -> Option<&'a Record> {
    cache.records.get(&candidate.id).filter(|r| r.listing == candidate.listing && r.fingerprint == candidate.fingerprint)
}
fn promote(item: &mut LibraryItem, kind: &str) -> bool {
    if item.kind == "Asset" && matches!(kind, "Project" | "Plugin") {
        item.kind = kind.into(); true
    } else { false }
}
fn distribution_kind(value: &str) -> Option<&'static str> {
    if value.len() > 64 { return None; }
    let compact: String = value.trim().to_ascii_lowercase().chars().filter(|c| !matches!(c, '_' | '-' | ' ')).collect();
    match compact.as_str() {
        "assetpack" => Some("Asset"),
        "completeproject" => Some("Project"),
        "codeplugin" | "engineplugin" => Some("Plugin"),
        _ => None,
    }
}
fn metadata_kind(value: &serde_json::Value) -> Option<String> {
    if value["assetFormatType"]["code"].as_str()? != "unreal-engine" { return None; }
    let root=value.get("distributionMethod").filter(|value|!value.is_null());
    let nested=value.pointer("/techDetails/gameSystem/distributionMethod").filter(|value|!value.is_null());
    let root=match root {Some(value)=>Some(distribution_kind(value.as_str()?)?),None=>None};
    let nested=match nested {Some(value)=>Some(distribution_kind(value.as_str()?)?),None=>None};
    match (root,nested) {
        (Some(root),Some(nested)) if root!=nested=>None,
        (Some(kind),_)|(_,Some(kind))=>Some(kind.into()),
        _=>None,
    }
}
fn valid_base(base: &str) -> bool {
    if base == FAB { return true; }
    #[cfg(test)] {
        if let Ok(url) = reqwest::Url::parse(base) {
            return url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
                && url.path() == "/" && url.query().is_none() && url.fragment().is_none() && url.username().is_empty() && url.password().is_none();
        }
    }
    false
}
enum Primary { Confirmed(String), Missing }
async fn fetch_kind(client: reqwest::Client, token: String, base: String, candidate: &Candidate, budget: Arc<AtomicUsize>, legacy_slots: Arc<tokio::sync::Semaphore>, legacy_attempts: Arc<AtomicUsize>) -> Option<String> {
    let endpoint = format!("{}/i/listings/{}/asset-formats/unreal-engine", base.trim_end_matches('/'), candidate.listing);
    // Callers supply a shared client with redirect Policy::none, as for Fab library sync.
    let primary = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let mut response = client.get(&endpoint).bearer_auth(&token).send().await.ok()?;
        if response.url().as_str() != endpoint { return None; }
        if response.status() == reqwest::StatusCode::NOT_FOUND { return Some(Primary::Missing); }
        if !response.status().is_success() || response.url().as_str() != endpoint
            || response.content_length().is_some_and(|n| n > MAX_RESPONSE as u64) { return None; }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > MAX_RESPONSE || budget.fetch_add(chunk.len(), Ordering::Relaxed).saturating_add(chunk.len()) > MAX_TRANSFER { return None; }
            bytes.extend_from_slice(&chunk);
        }
        Some(Primary::Confirmed(metadata_kind(&serde_json::from_slice::<serde_json::Value>(&bytes).ok()?)?))
    }).await.ok().flatten();
    match primary {
        Some(Primary::Confirmed(kind)) => Some(kind),
        Some(Primary::Missing) => {
            let asset = candidate.legacy.as_ref()?;
            let _permit = legacy_slots.acquire().await.ok()?;
            if budget.load(Ordering::Relaxed) >= MAX_TRANSFER { return None; }
            legacy_attempts.fetch_add(1, Ordering::Relaxed);
            legacy::classify(&client, &token, &asset.namespace, &candidate.id, &asset.artifact, budget).await
        }
        None => None,
    }
}

pub(crate) fn apply_cached(account: &str, items: &mut [LibraryItem]) -> usize {
    apply_cached_at(&crate::storage::cache().join("libraries"), account, items)
}
#[cfg(feature = "engine-test-cli")]
pub(crate) fn legacy_rejected_hosts() -> Vec<String> { legacy::rejected_hosts() }
#[cfg(feature="engine-test-cli")]
pub(crate) fn legacy_rejected_formats() -> Vec<String> { legacy::rejected_formats() }
#[cfg(feature="engine-test-cli")]
pub(crate) fn legacy_diagnostics() -> serde_json::Value { legacy::diagnostics() }
fn apply_cached_at(base: &Path, account: &str, items: &mut [LibraryItem]) -> usize {
    let Some(account) = account_key(account) else { return 0; };
    let cache = read_cache(base, &account);
    let mut changed = 0;
    for (index, item) in items.iter_mut().enumerate() {
        if item.declared_kind() != "Asset" { continue; }
        if let Some(candidate) = candidate(index, item) {
            if let Some(record) = matching(&cache, &candidate) { changed += usize::from(promote(item, &record.kind)); }
        }
    }
    changed
}

pub(crate) async fn enrich(client: &reqwest::Client, token: &str, account: &str, items: &mut [LibraryItem], progress: impl FnMut(usize, usize, usize)) -> Stats {
    enrich_at(client, token, account, items, &crate::storage::cache().join("libraries"), FAB, progress).await
}
/// The alternate cache directory is for isolated acceptance evidence; runtime uses `enrich`.
pub(crate) async fn enrich_at(client: &reqwest::Client, token: &str, account: &str, items: &mut [LibraryItem], cache_base: &Path, endpoint_base: &str, mut progress: impl FnMut(usize, usize, usize)) -> Stats {
    let mut stats = Stats { total: items.iter().filter(|item| item.declared_kind() == "Asset").count(), ..Stats::default() };
    progress(0, stats.total, 0);
    let Some(account) = account_key(account).filter(|_| valid_base(endpoint_base) && !token.is_empty()) else {
        stats.done = stats.total; stats.unresolved = stats.total; progress(stats.done, stats.total, stats.unresolved); return stats;
    };
    let timestamp = now();
    let mut cache = read_cache(cache_base, &account);
    let owned: BTreeSet<_> = items.iter().map(|item| item.id.as_str()).collect();
    cache.records.retain(|id, _| owned.contains(id.as_str()));
    let mut pending = VecDeque::new();
    for (index, item) in items.iter_mut().enumerate() {
        item.ensure_catalog_kind();
        if item.declared_kind() != "Asset" { continue; }
        let Some(candidate) = candidate(index, item) else { stats.done += 1; stats.unresolved += 1; continue; };
        if let Some(record) = matching(&cache, &candidate) {
            promote(item, &record.kind);
            if record.checked_at <= timestamp && timestamp.saturating_sub(record.checked_at) < MAX_AGE {
                item.kind = record.kind.clone();
                stats.done += 1; stats.cached += 1; continue;
            }
        }
        if pending.len() < MAX_RECORDS { pending.push_back(candidate); }
        else { stats.done += 1; stats.unresolved += 1; }
    }
    progress(stats.done, stats.total, stats.unresolved);
    let deadline = tokio::time::Instant::now() + SYNC_TIMEOUT;
    let budget = Arc::new(AtomicUsize::new(0));
    let legacy_slots = Arc::new(tokio::sync::Semaphore::new(2));
    let legacy_attempts = Arc::new(AtomicUsize::new(0));
    let mut active = tokio::task::JoinSet::new();
    let mut changed = false;
    loop {
        while active.len() < 4 && !pending.is_empty() && tokio::time::Instant::now() < deadline && budget.load(Ordering::Relaxed) < MAX_TRANSFER {
            let candidate = pending.pop_front().unwrap();
            let client = client.clone(); let token = token.to_owned(); let base = endpoint_base.to_owned(); let budget = budget.clone();
            let legacy_slots = legacy_slots.clone(); let legacy_attempts = legacy_attempts.clone();
            stats.fetched += 1;
            active.spawn(async move { let kind = fetch_kind(client, token, base, &candidate, budget, legacy_slots, legacy_attempts).await; (candidate, kind) });
        }
        if active.is_empty() { break; }
        match tokio::time::timeout_at(deadline, active.join_next()).await {
            Ok(Some(Ok((candidate, kind)))) => {
                stats.done += 1;
                if let Some(kind) = kind {
                    // This pass precedes the caller's local-manifest overlay.
                    // A successful Asset confirmation replaces stale sidecar hints.
                    items[candidate.index].kind = kind.clone();
                    if cache.records.contains_key(&candidate.id) || cache.records.len() < MAX_RECORDS {
                        cache.records.insert(candidate.id, Record { listing: candidate.listing, fingerprint: candidate.fingerprint, kind, checked_at: timestamp }); changed = true;
                    }
                } else { stats.unresolved += 1; }
                progress(stats.done, stats.total, stats.unresolved);
            }
            Ok(Some(Err(_))) => { stats.done += 1; stats.unresolved += 1; progress(stats.done, stats.total, stats.unresolved); }
            _ => { active.abort_all(); break; }
        }
    }
    active.abort_all();
    while active.join_next().await.is_some() {}
    stats.legacy_fetched = legacy_attempts.load(Ordering::Relaxed);
    let unfinished = stats.total.saturating_sub(stats.done);
    stats.unresolved += unfinished; stats.done = stats.total;
    progress(stats.done, stats.total, stats.unresolved);
    if changed { save_cache(cache_base, &account, &cache); }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, sync::Mutex};
    const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
    const UID: &str = "4c2581cf-93e3-421f-b403-05b180ecb25a";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self { let path = std::env::temp_dir().join(format!("um-fab-types-{}", uuid::Uuid::new_v4())); fs::create_dir(&path).unwrap(); Self(path) }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn item(id: &str, uid: &str) -> LibraryItem {
        serde_json::from_value(serde_json::json!({"id":id,"title":"Synthetic material","kind":"Asset","catalogKind":"Asset","engineVersions":["5.8"],"categories":[],"source":"Fab","namespace":"ue","versions":[{"artifactId":"fixture-artifact","engines":["5.8"],"platforms":["Windows"]}],"listingUrl":format!("https://www.fab.com/listings/{uid}")})).unwrap()
    }
    fn metadata(method: &str) -> serde_json::Value { serde_json::json!({"assetFormatType":{"code":"unreal-engine"},"distributionMethod":method}) }
    #[test] fn captured_complete_project_and_explicit_code_plugin() {
        let mut value = metadata("complete_project"); value["techDetails"] = serde_json::json!({"gameSystem":{"distributionMethod":"complete_project"}});
        assert_eq!(metadata_kind(&value).as_deref(), Some("Project"));
        assert_eq!(metadata_kind(&metadata("CODE_PLUGIN")).as_deref(), Some("Plugin"));
        assert_eq!(metadata_kind(&metadata("asset-pack")).as_deref(), Some("Asset"));
        value["techDetails"]["gameSystem"]["distributionMethod"] = "asset_pack".into(); assert!(metadata_kind(&value).is_none());
        for method in ["projectile", "ADD_TO_PROJECT", "plugin collection", "not_a_complete_project"] { assert!(metadata_kind(&metadata(method)).is_none()); }
        let fake = serde_json::json!({"assetFormatType":{"code":"unity"},"distributionMethod":"complete_project","description":"Complete Project .uproject"}); assert!(metadata_kind(&fake).is_none());
        assert!(metadata_kind(&serde_json::json!({"description":"Complete Project","technicalDetails":"Plugin .uplugin"})).is_none());
    }
    #[test] fn captured_nullable_root_uses_explicit_game_system_distribution(){
        let value=serde_json::json!({"assetFormatType":{"code":"unreal-engine"},"distributionMethod":null,"techDetails":{"gameSystem":{"distributionMethod":"asset_pack"}}});
        assert_eq!(metadata_kind(&value).as_deref(),Some("Asset"));
        let mut unknown=value.clone();unknown["distributionMethod"]="future_unknown".into();assert!(metadata_kind(&unknown).is_none());
        let mut absent=value;absent["techDetails"]["gameSystem"]["distributionMethod"]=serde_json::Value::Null;assert!(metadata_kind(&absent).is_none());
    }
    #[test] fn exact_listing_origin_and_stable_artifact_fingerprint() {
        assert_eq!(listing_uid(&format!("https://www.fab.com/en-US/listings/{UID}?ref=test")).as_deref(), Some(UID));
        for value in [format!("https://fab.com.evil.test/listings/{UID}"), format!("http://www.fab.com/listings/{UID}"), format!("https://www.fab.com/other/listings/{UID}"), format!("https://user@www.fab.com/listings/{UID}")] { assert!(listing_uid(&value).is_none()); }
        let first = item("a", UID); let mut second = first.clone(); second.versions[0].engines.push("5.7".into()); assert_ne!(fingerprint(&first), fingerprint(&second));
        let mut reordered = second.clone(); reordered.versions[0].engines.reverse(); assert_eq!(fingerprint(&second), fingerprint(&reordered));
        reordered.versions[0].artifact_id = "another".into(); assert_ne!(fingerprint(&second), fingerprint(&reordered));
    }
    #[test] fn legacy_uses_highest_windows_artifact_with_numeric_engine_order() {
        let mut selected = item("owned-id", UID);
        let mut version = selected.versions[0].clone(); version.artifact_id = "windows-5.9".into(); version.engines = vec!["UE_5.9".into()]; selected.versions.push(version.clone());
        version.artifact_id = "windows-5.10".into(); version.engines = vec!["5.10".into()]; selected.versions.push(version.clone());
        version.artifact_id = "linux-9".into(); version.engines = vec!["9.0".into()]; version.platforms = vec!["Linux".into()]; selected.versions.push(version);
        assert_eq!(legacy_asset(&selected).unwrap().artifact, "windows-5.10");
        selected.versions.reverse(); assert_eq!(legacy_asset(&selected).unwrap().artifact, "windows-5.10");
        selected.namespace = "../wrong".into(); assert!(legacy_asset(&selected).is_none());
        assert!(!identifier("..")); assert!(!identifier("bad/path"));
    }
    #[test] fn cache_bounds_identity_and_no_catalogue_mutation() {
        let fixture = Fixture::new(); let original = item("a", UID); let candidate = candidate(0, &original).unwrap();
        let mut cache = empty_cache(ACCOUNT); cache.records.insert("a".into(), Record { listing: candidate.listing, fingerprint: candidate.fingerprint, kind: "Project".into(), checked_at: now() });
        save_cache(&fixture.0, ACCOUNT, &cache);
        let mut items = vec![original.clone()]; assert_eq!(apply_cached_at(&fixture.0, ACCOUNT, &mut items), 1); assert_eq!(items[0].kind, "Project"); assert_eq!(items[0].catalog_kind, "Asset");
        items[0].kind = "Asset".into(); items[0].versions[0].artifact_id = "changed".into(); assert_eq!(apply_cached_at(&fixture.0, ACCOUNT, &mut items), 0);
        assert_eq!(apply_cached_at(&fixture.0, "ffffffffffffffffffffffffffffffff", &mut items), 0);
        assert!(decode_cache(&vec![b' '; MAX_CACHE + 1], ACCOUNT).is_none());
        cache.records.insert("bad".into(), Record { listing: "invalid".into(), fingerprint: "x".into(), kind: "Project".into(), checked_at: 0 }); assert!(decode_cache(&serde_json::to_vec(&cache).unwrap(), ACCOUNT).is_none());
        assert!(cache_path(&fixture.0, "../other").is_none());
    }
    #[tokio::test] async fn four_inflight_progress_failures_and_second_pass_cache_hits() {
        let fixture = Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0)); let live = Arc::new(AtomicUsize::new(0)); let maximum = Arc::new(AtomicUsize::new(0)); let seen = Arc::new(Mutex::new(Vec::new()));
        let server_requests = requests.clone(); let server_live = live.clone(); let server_max = maximum.clone(); let server_seen = seen.clone();
        let server = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for _ in 0..8 {
                let (mut stream, _) = listener.accept().unwrap(); let requests = server_requests.clone(); let live = server_live.clone(); let maximum = server_max.clone(); let seen = server_seen.clone();
                workers.push(std::thread::spawn(move || {
                    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap(); let mut data = [0; 8192]; let count = stream.read(&mut data).unwrap(); let request = String::from_utf8_lossy(&data[..count]);
                    requests.fetch_add(1, Ordering::SeqCst); let current = live.fetch_add(1, Ordering::SeqCst) + 1; maximum.fetch_max(current, Ordering::SeqCst); seen.lock().unwrap().push(request.lines().next().unwrap().to_owned());
                    std::thread::sleep(Duration::from_millis(40));
                    let lost = request.contains("00000000-0000-0000-0000-000000000008");
                    if !lost { let method = if request.contains("00000000-0000-0000-0000-000000000002") { "asset_pack" } else { "complete_project" }; let body = metadata(method).to_string(); let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body); }
                    live.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            for worker in workers { worker.join().unwrap(); }
        });
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let originals: Vec<_> = (1..=8).map(|n| item(&format!("asset-{n}"), &format!("00000000-0000-0000-0000-{n:012}"))).collect(); let mut items = originals.clone(); let mut progress = Vec::new();
        let stats = enrich_at(&client, "synthetic", ACCOUNT, &mut items, &fixture.0, &endpoint, |d,t,u| progress.push((d,t,u))).await;
        server.join().unwrap(); assert_eq!(stats.total, 8); assert_eq!(stats.done, 8); assert_eq!(stats.fetched, 8); assert_eq!(stats.unresolved, 1); assert_eq!(requests.load(Ordering::SeqCst), 8); assert!(maximum.load(Ordering::SeqCst) <= 4); assert!(maximum.load(Ordering::SeqCst) >= 2);
        assert_eq!(progress.last(), Some(&(8,8,1))); assert!(progress.windows(2).all(|p|p[0].0<=p[1].0)); assert!(seen.lock().unwrap().iter().all(|p|p.ends_with("/asset-formats/unreal-engine HTTP/1.1")));
        assert_eq!(items[1].kind, "Asset"); assert_eq!(items[7].kind, "Asset"); assert!(items.iter().all(|i|i.catalog_kind=="Asset"));
        let mut successful = originals[..7].to_vec(); let cached = enrich_at(&client, "synthetic", ACCOUNT, &mut successful, &fixture.0, &endpoint, |_,_,_|{}).await;
        assert_eq!(cached.cached, 7); assert_eq!(cached.fetched, 0); assert_eq!(cached.unresolved, 0); assert_eq!(requests.load(Ordering::SeqCst), 8);
    }
    #[tokio::test] async fn fresh_asset_confirmation_replaces_expired_project_hint_and_is_cached() {
        let fixture = Fixture::new(); let original = item("stale", UID); let candidate = candidate(0, &original).unwrap();
        let mut cache = empty_cache(ACCOUNT); cache.records.insert("stale".into(), Record { listing: candidate.listing, fingerprint: candidate.fingerprint, kind: "Project".into(), checked_at: now().saturating_sub(MAX_AGE + 1) }); save_cache(&fixture.0, ACCOUNT, &cache);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || { let (mut stream, _) = listener.accept().unwrap(); stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap(); let mut data = [0; 8192]; let _ = stream.read(&mut data).unwrap(); let body = metadata("asset_pack").to_string(); write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap(); });
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(); let mut items = vec![original];
        let first = enrich_at(&client, "synthetic", ACCOUNT, &mut items, &fixture.0, &endpoint, |_,_,_|{}).await; server.join().unwrap();
        assert_eq!(first.fetched, 1); assert_eq!(first.unresolved, 0); assert_eq!(items[0].kind, "Asset"); assert_eq!(items[0].catalog_kind, "Asset");
        items[0].kind = "Project".into();
        let second = enrich_at(&client, "synthetic", ACCOUNT, &mut items, &fixture.0, &endpoint, |_,_,_|{}).await;
        assert_eq!(second.cached, 1); assert_eq!(second.fetched, 0); assert_eq!(items[0].kind, "Asset"); assert_eq!(items[0].catalog_kind, "Asset");
    }
    #[tokio::test] async fn authorization_rate_limit_and_server_errors_never_enter_legacy_fallback() {
        let fixture = Fixture::new(); let listener = TcpListener::bind("127.0.0.1:0").unwrap(); let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || { for status in [401,403,429,500] { let (mut stream, _) = listener.accept().unwrap(); stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap(); let mut data = [0;8192]; let _ = stream.read(&mut data).unwrap(); write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(); } });
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(); let mut items:Vec<_>=(0..4).map(|n|item(&format!("owned-{n}"),UID)).collect();
        let stats = enrich_at(&client,"synthetic",ACCOUNT,&mut items,&fixture.0,&endpoint,|_,_,_|{}).await; server.join().unwrap();
        assert_eq!(stats.fetched,4); assert_eq!(stats.legacy_fetched,0); assert_eq!(stats.unresolved,4); assert!(items.iter().all(|item|item.kind=="Asset"));
    }
}

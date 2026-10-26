use std::{collections::{BTreeMap, BTreeSet}, fs, io::{Cursor, Read}, path::{Path, PathBuf}, sync::LazyLock, time::{Duration, SystemTime, UNIX_EPOCH}};
use serde::{Serialize, Deserialize};
use sha1::{Digest, Sha1};
use base64::Engine;
use super::epic::LibraryItem;


const TTL: u64 = 24 * 60 * 60;
static IMAGE_SLOTS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| tokio::sync::Semaphore::new(3));
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CachedLibrary {
    pub items: Vec<LibraryItem>, pub cached_at: u64, pub stale: bool,
    // Public content types learned from a validated selected manifest. This
    // does not add ownership, replace catalogue fields, or refresh sync time.
    #[serde(default, skip_serializing_if="BTreeMap::is_empty")]
    pub(crate) manifest_kinds: BTreeMap<String, String>,
}
fn reconcile(cache: &mut CachedLibrary, timestamp: u64) {
    let ids: BTreeSet<_> = cache.items.iter().map(|item| item.id.as_str()).collect();
    cache.manifest_kinds.retain(|id,kind| ids.contains(id.as_str()) && matches!(kind.as_str(), "Project" | "Plugin"));
    for item in &mut cache.items {
        item.ensure_catalog_kind();
        item.kind=item.declared_kind().to_owned();
    }
    super::epic::apply_known_content_kinds(&mut cache.items, &cache.manifest_kinds);
    cache.stale=timestamp.saturating_sub(cache.cached_at)>=TTL || cache.cached_at>timestamp || cache.items.iter().any(|item|item.info_version<super::epic::LIBRARY_INFO_VERSION);
}
fn decode(bytes: &[u8], timestamp: u64) -> Option<CachedLibrary> {
    let mut cache: CachedLibrary=serde_json::from_slice(bytes).ok()?;
    reconcile(&mut cache,timestamp);
    Some(cache)
}
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
fn library_path(account: &str) -> Result<PathBuf, String> {
    if account.is_empty() || !account.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Некорректный аккаунт кэша".into()); }
    Ok(crate::storage::cache().join("libraries").join(format!("{account}.json")))
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent=path.parent().ok_or("Некорректный каталог кэша")?;
    fs::create_dir_all(parent).map_err(|_| "Не удалось создать кэш")?;
    let temporary=parent.join(format!("{}.tmp",uuid::Uuid::new_v4().simple()));
    fs::write(&temporary,bytes).map_err(|_| "Не удалось записать кэш")?;
    fs::rename(temporary,path).map_err(|_| "Не удалось обновить кэш".into())
}
pub fn read(account: &str) -> Result<Option<CachedLibrary>, String> {
    let path=library_path(account)?;
    let file=match fs::File::open(path) { Ok(f)=>f, Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None), Err(_)=>return Err("Не удалось прочитать кэш библиотеки".into()) };
    let mut bytes=Vec::new(); file.take(64*1024*1024).read_to_end(&mut bytes).map_err(|_| "Не удалось прочитать кэш")?;
    Ok(decode(&bytes,now()))
}
pub fn save(account: &str, items: &[LibraryItem]) -> Result<(), String> {
    let manifest_kinds=read(account).ok().flatten().map(|cache|cache.manifest_kinds).unwrap_or_default();
    let timestamp=now();
    let mut cache=CachedLibrary { items:items.to_vec(),cached_at:timestamp,stale:false,manifest_kinds };
    reconcile(&mut cache,timestamp);
    let bytes=serde_json::to_vec(&cache).map_err(|_| "Не удалось подготовить кэш")?;
    write_atomic(&library_path(account)?,&bytes)
}
fn learn_manifest_kind(cache: &mut CachedLibrary, items: &[LibraryItem], id: &str, kind: &str, timestamp: u64) -> bool {
    if !matches!(kind,"Project"|"Plugin") || !items.iter().any(|item|item.id==id) { return false; }
    if cache.manifest_kinds.get(id).is_some_and(|known|known==kind || known=="Project") { return false; }
    if !cache.items.iter().any(|item|item.id==id) { cache.items=items.to_vec(); }
    cache.manifest_kinds.insert(id.to_owned(),kind.to_owned());
    reconcile(cache,timestamp);
    true
}
pub(crate) fn record_manifest_kind(account: &str, items: &[LibraryItem], id: &str, kind: &str) -> Result<(), String> {
    if !matches!(kind,"Project"|"Plugin") || !items.iter().any(|item|item.id==id) { return Ok(()); }
    let mut cache=read(account)?.unwrap_or_else(||CachedLibrary {items:items.to_vec(),cached_at:0,stale:true,manifest_kinds:BTreeMap::new()});
    if !learn_manifest_kind(&mut cache,items,id,kind,now()) { return Ok(()); }
    let bytes=serde_json::to_vec(&cache).map_err(|_| "Не удалось подготовить кэш типа материала")?;
    write_atomic(&library_path(account)?,&bytes)
}
pub(crate) fn image_allowed(url: &reqwest::Url) -> bool {
    let host=url.host_str().unwrap_or("");
    url.scheme()=="https" && url.port_or_known_default()==Some(443) && ["epicgames.com","unrealengine.com","fab.com","sketchfab.com","cloudfront.net"].iter().any(|base| host==*base||host.ends_with(&format!(".{base}")))
}
#[derive(Default, Serialize, Deserialize)]
struct CoverMetadata { url: String, etag: Option<String>, modified: Option<String>, revision: u64 }
fn needs_check(meta: Option<&CoverMetadata>, url: &str, revision: u64) -> bool {
    meta.is_none_or(|m| m.url != url || (revision != 0 && m.revision != revision))
}
async fn refresh_cover(path: &Path, meta_path: &Path, url: &str, revision: u64, cached: bool, meta: Option<&CoverMetadata>, large: bool) -> Result<(), String> {
    let parsed=reqwest::Url::parse(url).map_err(|_| "Некорректная обложка")?;
    if !image_allowed(&parsed) { return Err("Источник обложки не поддерживается".into()); }
    let client=reqwest::Client::builder().timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "Не удалось загрузить обложку")?;
    let mut request=client.get(parsed);
    if let Some(m)=meta.filter(|m| cached && m.url==url) {
        if let Some(etag)=&m.etag { request=request.header(reqwest::header::IF_NONE_MATCH,etag); }
        else if let Some(modified)=&m.modified { request=request.header(reqwest::header::IF_MODIFIED_SINCE,modified); }
    }
    let mut response=request.send().await.map_err(|_| "Не удалось загрузить обложку")?;
    let unchanged=response.status()==reqwest::StatusCode::NOT_MODIFIED;
    if unchanged && (!cached || meta.is_none_or(|m| m.url!=url)) { return Err("Некорректный ответ сервера обложек".into()); }
    if !unchanged && !response.status().is_success() { return Err("Обложка недоступна".into()); }
    let header=|name| response.headers().get(name).and_then(|v|v.to_str().ok()).map(str::to_owned);
    let metadata=CoverMetadata { url:url.to_owned(), etag:header(reqwest::header::ETAG).or_else(|| if unchanged {meta.and_then(|m|m.etag.clone())}else{None}), modified:header(reqwest::header::LAST_MODIFIED).or_else(|| if unchanged {meta.and_then(|m|m.modified.clone())}else{None}), revision };
    if !unchanged {
        let mut raw=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|_| "Обложка недоступна")? { if raw.len()+chunk.len()>8*1024*1024 { return Err("Обложка слишком велика".into()); } raw.extend_from_slice(&chunk); }
        let bytes=tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>,String> {
            let mut reader=image::ImageReader::new(Cursor::new(raw)).with_guessed_format().map_err(|_| "Неизвестный формат обложки")?;
            let mut limits=image::Limits::default();limits.max_image_width=Some(8192);limits.max_image_height=Some(8192);limits.max_alloc=Some(64*1024*1024);reader.limits(limits);
            let picture=reader.decode().map_err(|_| "Не удалось прочитать обложку")?.thumbnail(if large {1600}else{192},if large {1200}else{108}).to_rgb8();
            let mut output=Cursor::new(Vec::new());picture.write_to(&mut output,image::ImageFormat::Jpeg).map_err(|_| "Не удалось сохранить обложку")?; Ok(output.into_inner())
        }).await.map_err(|_| "Не удалось подготовить обложку")??;
        write_atomic(path,&bytes)?;
    }
    write_atomic(meta_path,&serde_json::to_vec(&metadata).map_err(|_| "Не удалось сохранить сведения об обложке")?)
}
pub async fn thumbnail(id: String, url: String, revision: u64) -> Result<String,String> {
    cached_image(id,url,revision,false).await
}
pub async fn cached_image(id: String, url: String, revision: u64, large:bool) -> Result<String,String> {
    let path=crate::storage::cache().join(if large {"gallery"}else{"covers"}).join(format!("{:x}.jpg",Sha1::digest(id.as_bytes())));
    let meta_path=path.with_extension("json");
    let _slot=IMAGE_SLOTS.acquire().await.map_err(|_| "Кэш обложек недоступен")?;
    let cached=path.exists();
    let meta:Option<CoverMetadata>=fs::read(&meta_path).ok().and_then(|b|serde_json::from_slice(&b).ok());
    if !cached || needs_check(meta.as_ref(),&url,revision) {
        // Keep the last good image when offline or when a replacement is broken.
        if let Err(error)=refresh_cover(&path,&meta_path,&url,revision,cached,meta.as_ref(),large).await { if !cached {return Err(error);} }
    }
    let bytes=fs::read(&path).map_err(|_| "Не удалось прочитать обложку")?;
    if bytes.len()>8*1024*1024 { return Err("Повреждён кэш обложки".into()); }
    Ok(format!("data:image/jpeg;base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes)))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item(id:&str,kind:&str,version:u8)->LibraryItem {
        serde_json::from_value(serde_json::json!({"id":id,"title":"Synthetic item","kind":kind,"engineVersions":[],"categories":[],"source":"Fab","namespace":"fixture","versions":[],"infoVersion":version})).unwrap()
    }
    fn cache(items:Vec<LibraryItem>,cached_at:u64)->CachedLibrary {
        CachedLibrary {items,cached_at,stale:false,manifest_kinds:BTreeMap::new()}
    }
    #[test] fn legacy_library_cache_migrates_types_without_renewing_sync_time() {
        let mut value=serde_json::to_value(cache(vec![item("project","Project",1)],1000)).unwrap();
        value["items"][0].as_object_mut().unwrap().remove("catalogKind");
        value.as_object_mut().unwrap().remove("manifestKinds");
        let migrated=decode(&serde_json::to_vec(&value).unwrap(),1001).unwrap();
        assert!(migrated.stale);assert_eq!(migrated.cached_at,1000);
        assert_eq!(migrated.items[0].kind,"Project");assert_eq!(migrated.items[0].declared_kind(),"Project");
        assert!(migrated.manifest_kinds.is_empty());
    }
    #[test] fn selected_manifest_kind_survives_cache_reload_without_changing_catalogue_kind() {
        let items=vec![item("selected","Asset",super::super::epic::LIBRARY_INFO_VERSION)];
        let mut saved=cache(items.clone(),1000);
        assert!(learn_manifest_kind(&mut saved,&items,"selected","Project",1001));
        assert_eq!(saved.cached_at,1000);assert!(!saved.stale);
        assert_eq!(saved.items[0].kind,"Project");assert_eq!(saved.items[0].declared_kind(),"Asset");
        let encoded=serde_json::to_vec(&saved).unwrap();let loaded=decode(&encoded,1002).unwrap();
        assert_eq!(loaded.items.len(),1);assert_eq!(loaded.items[0].id,"selected");
        assert_eq!(loaded.items[0].kind,"Project");assert_eq!(loaded.items[0].declared_kind(),"Asset");
        assert_eq!(loaded.manifest_kinds.get("selected").map(String::as_str),Some("Project"));
        assert!(decode(&encoded,1000+TTL+1).unwrap().stale);
    }
    #[test] fn content_proof_is_bounded_to_owned_items_and_positive_canonical_kinds() {
        let items=vec![item("owned","Asset",super::super::epic::LIBRARY_INFO_VERSION)];let mut saved=cache(items.clone(),1000);
        assert!(!learn_manifest_kind(&mut saved,&items,"missing","Project",1001));
        assert!(!learn_manifest_kind(&mut saved,&items,"owned","Unexpected",1001));
        assert!(!learn_manifest_kind(&mut saved,&items,"owned","Asset",1001));
        saved.manifest_kinds=BTreeMap::from([("missing".into(),"Project".into()),("owned".into(),"Unexpected".into())]);
        let loaded=decode(&serde_json::to_vec(&saved).unwrap(),1001).unwrap();
        assert!(loaded.manifest_kinds.is_empty());assert_eq!(loaded.items.len(),1);assert_eq!(loaded.items[0].kind,"Asset");
    }
    #[test] fn display_only_overlay_cannot_replace_per_artifact_catalogue_kind() {
        let mut project=item("project","Project",super::super::epic::LIBRARY_INFO_VERSION);project.catalog_kind="Asset".into();
        let plugin=item("plugin","Plugin",super::super::epic::LIBRARY_INFO_VERSION);
        let mut saved=cache(vec![project,plugin],1000);saved.manifest_kinds.insert("plugin".into(),"Project".into());
        let loaded=decode(&serde_json::to_vec(&saved).unwrap(),1001).unwrap();
        assert_eq!(loaded.items[0].kind,"Asset");assert_eq!(loaded.items[0].declared_kind(),"Asset");
        assert_eq!(loaded.items[1].kind,"Plugin");assert_eq!(loaded.items[1].declared_kind(),"Plugin");
    }
    #[test] fn known_project_evidence_is_not_demoted_by_another_plugin_artifact() {
        let items=vec![item("selected","Asset",super::super::epic::LIBRARY_INFO_VERSION)];let mut saved=cache(items.clone(),1000);
        assert!(learn_manifest_kind(&mut saved,&items,"selected","Project",1001));
        assert!(!learn_manifest_kind(&mut saved,&items,"selected","Plugin",1002));
        assert_eq!(saved.manifest_kinds.get("selected").map(String::as_str),Some("Project"));
        assert_eq!(saved.cached_at,1000);
    }
    #[test] fn cover_refresh_policy() {
        let meta=CoverMetadata {url:"https://cdn1.epicgames.com/a.jpg".into(),revision:7,..Default::default()};
        assert!(!needs_check(Some(&meta),&meta.url,0)); // restart from local library
        assert!(!needs_check(Some(&meta),&meta.url,7)); // another tile in same sync
        assert!(needs_check(Some(&meta),&meta.url,8)); // revalidate same URL
        assert!(needs_check(Some(&meta),"https://cdn1.epicgames.com/b.jpg",0));
        assert!(needs_check(None,&meta.url,0)); // migrate legacy cache
    }
    #[test] fn cache_keys_and_cover_origins() {
        assert!(library_path("../other").is_err());assert_ne!(library_path("aa").unwrap(),library_path("bb").unwrap());
        assert!(image_allowed(&"https://cdn1.epicgames.com/a.jpg".parse().unwrap()));
        for url in ["http://cdn1.epicgames.com/a", "https://epicgames.com.evil/a", "https://localhost/a", "https://127.0.0.1/a"] { assert!(!image_allowed(&url.parse().unwrap())); }
    }
}


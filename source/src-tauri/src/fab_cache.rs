use std::{fs, io::{Cursor, Read}, path::{Path, PathBuf}, sync::LazyLock, time::{Duration, SystemTime, UNIX_EPOCH}};
use serde::{Serialize, Deserialize};
use sha1::{Digest, Sha1};
use base64::Engine;
use super::epic::LibraryItem;


const TTL: u64 = 24 * 60 * 60;
static IMAGE_SLOTS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| tokio::sync::Semaphore::new(3));
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CachedLibrary { pub items: Vec<LibraryItem>, pub cached_at: u64, pub stale: bool }
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
    let mut cache:CachedLibrary=match serde_json::from_slice(&bytes) { Ok(c)=>c,Err(_)=>return Ok(None) };
    cache.stale=now().saturating_sub(cache.cached_at)>=TTL || cache.cached_at>now() || cache.items.iter().any(|i|i.info_version<1);
    Ok(Some(cache))
}
pub fn save(account: &str, items: &[LibraryItem]) -> Result<(), String> {
    let bytes=serde_json::to_vec(&CachedLibrary { items:items.to_vec(),cached_at:now(),stale:false }).map_err(|_| "Не удалось подготовить кэш")?;
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


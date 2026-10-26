use egs_api::{EpicGames, api::types::account::UserData};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::PathBuf, time::Duration};

use tokio::sync::Mutex;

pub(crate) const AUTH_REQUIRED: &str = "UM_EPIC_AUTH_REQUIRED";
pub(crate) const LIBRARY_INFO_VERSION: u8 = 2;
fn api_failure(error: egs_api::api::error::EpicAPIError, fallback: &str) -> String {
    match error {
        egs_api::api::error::EpicAPIError::HttpError { status, .. } if status == reqwest::StatusCode::UNAUTHORIZED => AUTH_REQUIRED.into(),
        _ => fallback.into(),
    }
}

// Login resets the shared website profile. Reserve it before either flow can
// await so a pending login cannot replace cookies underneath redemption.
static WEB_FLOW:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
pub(crate) struct WebFlowGuard(());
impl Drop for WebFlowGuard {
    fn drop(&mut self){WEB_FLOW.store(false,std::sync::atomic::Ordering::SeqCst);}
}
pub(crate) fn reserve_web_flow()->Result<WebFlowGuard,String>{
    WEB_FLOW.compare_exchange(false,true,std::sync::atomic::Ordering::SeqCst,std::sync::atomic::Ordering::SeqCst)
        .map(|_|WebFlowGuard(())).map_err(|_|"Сначала закройте окно Epic Games".into())
}

pub struct EpicState(Mutex<Session>);
struct Session { api: EpicGames, loaded: bool, remember: bool, library: Vec<LibraryItem> }
impl Default for EpicState {
    fn default() -> Self { Self(Mutex::new(Session { api: EpicGames::new(), loaded: false, remember: false, library: Vec::new() })) }
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct AccountStatus { signed_in: bool, display_name: Option<String> }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct LibraryItem {
    pub(crate) id: String, pub(crate) title: String, pub(crate) kind: String, engine_versions: Vec<String>,
    #[serde(default)]
    pub(crate) catalog_kind: String,
    categories: Vec<String>, source: String,
    namespace: String, pub(crate) versions: Vec<AssetVersion>,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    pub(crate) info_version: u8,
    #[serde(default)]
    pub(crate) images: Vec<String>,
    #[serde(default)]
    pub(crate) listing_url: String,
}
impl LibraryItem {
    pub(crate) fn declared_kind(&self) -> &str {
        if matches!(self.catalog_kind.as_str(), "Asset" | "Project" | "Plugin") { &self.catalog_kind }
        else if matches!(self.kind.as_str(), "Asset" | "Project" | "Plugin") { &self.kind }
        else { "Asset" }
    }
    pub(crate) fn ensure_catalog_kind(&mut self) {
        if !matches!(self.catalog_kind.as_str(), "Asset" | "Project" | "Plugin") {
            self.catalog_kind = if matches!(self.kind.as_str(), "Asset" | "Project" | "Plugin") { self.kind.clone() } else { "Asset".into() };
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct AssetVersion { pub(crate) artifact_id: String, pub(crate) engines: Vec<String>, pub(crate) platforms: Vec<String> }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Library { items: Vec<LibraryItem>, scope: &'static str }

fn secret_path(_app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::storage::root().join("Account/epic-session.dpapi"))
}

#[cfg(windows)]
fn protect(data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| "Слишком большой сеанс")?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // DPAPI defaults to the current Windows user; plaintext never goes to disk.
    let result = unsafe {
        if decrypt { CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if result == 0 { return Err("Windows не удалось защитить или прочитать сохранённый сеанс".into()); }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData.cast()); }
    Ok(bytes)
}
#[cfg(not(windows))]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>, String> { Err("Сохранение входа реализовано только для Windows".into()) }

fn save(app: &tauri::AppHandle, session: &Session) -> Result<(), String> {
    if !session.remember { return Ok(()); }
    let path = secret_path(app)?;
    let mut plain = serde_json::to_vec(&session.api.user_details()).map_err(|_| "Не удалось подготовить сеанс")?;
    let encrypted = protect(&plain, false);
    plain.fill(0);
    let encrypted = encrypted?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|_| "Не удалось создать каталог сеанса")?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, encrypted).map_err(|_| "Не удалось сохранить сеанс")?;
    fs::rename(temporary, path).map_err(|_| "Не удалось обновить сохранённый сеанс".into())
}
fn forget(app: &tauri::AppHandle) -> Result<(), String> {
    match fs::remove_file(secret_path(app)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Не удалось удалить сохранённый сеанс".into()),
    }
}
fn load(app: &tauri::AppHandle, session: &mut Session) -> Result<(), String> {
    if session.loaded { return Ok(()); }
    session.loaded = true;
    let bytes = match fs::read(secret_path(app)?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Не удалось прочитать сохранённый сеанс".into()),
    };
    let mut plain = protect(&bytes, true)?;
    let details = serde_json::from_slice::<UserData>(&plain);
    plain.fill(0);
    session.api.set_user_details(details.map_err(|_| "Повреждён сохранённый сеанс; выполните вход заново")?);
    session.remember = true;
    Ok(())
}
async fn ensure(app: &tauri::AppHandle, session: &mut Session) -> Result<bool, String> {
    let _update_operation=crate::app_updates::operation()?;
    load(app, session)?;
    if session.api.is_logged_in() { return Ok(true); }
    if session.api.user_details().refresh_token().is_none() { return Ok(false); }
    let result = tokio::time::timeout(Duration::from_secs(40), session.api.try_login()).await
        .map_err(|_| "Epic не ответил вовремя. Повторите попытку")?
        .map_err(|error| api_failure(error, "Не удалось восстановить вход Epic. Проверьте сеть или войдите заново"))?;
    if result { save(app, session)?; }
    Ok(result)
}

#[tauri::command]
pub async fn epic_status(app: tauri::AppHandle, state: tauri::State<'_, EpicState>) -> Result<AccountStatus, String> {
    let mut session = state.0.lock().await;
    let signed_in = ensure(&app, &mut session).await?;
    Ok(AccountStatus { signed_in, display_name: if signed_in { session.api.user_details().display_name } else { None } })
}

#[tauri::command]
pub async fn epic_begin_login(app: tauri::AppHandle, state: tauri::State<'_, EpicState>, remember: bool) -> Result<AccountStatus, String> {
    if super::epic_redeem::is_open(){return Err("Сначала закройте окно Epic Games".into());}
    let _web_flow=reserve_web_flow()?;
    let code = super::epic_login::authorize(&app, remember).await?;
    epic_complete_login(app, state, code, remember).await
}

pub(crate) fn parse_code(value: &str) -> Result<String, String> {
    if value.len() > 8192 { return Err("Слишком длинный код входа".into()); }
    let input = value.trim();
    let code = if input.starts_with('{') {
        let json: serde_json::Value = serde_json::from_str(input).map_err(|_| "Некорректный ответ Epic")?;
        json.get("authorizationCode").and_then(|v| v.as_str()).ok_or("В ответе Epic нет authorizationCode")?.to_string()
    } else { input.trim_matches('"').to_string() };
    if !(16..=512).contains(&code.len()) || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("Epic вернул некорректный результат входа".into());
    }
    Ok(code)
}

pub async fn epic_complete_login(app: tauri::AppHandle, state: tauri::State<'_, EpicState>, code: String, remember: bool) -> Result<AccountStatus, String> {
    let _update_operation=crate::app_updates::operation()?;
    let code = parse_code(&code)?;
    let mut session = state.0.lock().await;
    if super::epic_redeem::is_open(){return Err("Сначала закройте окно Epic Games".into());}
    let mut next = Session { api: EpicGames::new(), loaded: true, remember, library: Vec::new() };
    let success = tokio::time::timeout(Duration::from_secs(40), next.api.try_auth_code(None, Some(code))).await
        .map_err(|_| "Epic не ответил вовремя. Повторите вход")?
        .map_err(|_| "Epic отклонил вход или недоступен. Повторите вход")?;
    if !success { return Err("Epic не подтвердил вход. Повторите попытку".into()); }
    if remember { save(&app, &next)?; } else { forget(&app)?; }
    let display_name = next.api.user_details().display_name;
    *session = next;
    Ok(AccountStatus { signed_in: true, display_name })
}

#[tauri::command]
pub async fn epic_logout(app: tauri::AppHandle, state: tauri::State<'_, EpicState>) -> Result<(), String> {
    let _update_operation=crate::app_updates::operation()?;
    let mut session = state.0.lock().await;
    if super::epic_redeem::is_open(){return Err("Сначала закройте окно Epic Games".into());}
    forget(&app)?;
    let mut old = std::mem::replace(&mut session.api, EpicGames::new());
    session.loaded = true; session.remember = false;
    session.library.clear();
    drop(session);
    // Local sign-out succeeds even if the remote service is offline.
    let _ = tokio::time::timeout(Duration::from_secs(10), old.logout()).await;
    Ok(())
}

// Only the explicit distribution field is a catalogue type hint. Product names,
// descriptions and broad categories must not turn an asset into a project.
fn declared_library_kind(method: &str) -> &'static str {
    if method.len() > 64 { return "Asset"; }
    let compact: String = method.trim().to_ascii_lowercase().chars()
        .filter(|c| !matches!(c, '_' | '-' | ' ')).collect();
    match compact.as_str() {
        "engineplugin" | "plugin" => "Plugin",
        "completeproject" | "project" => "Project",
        _ => "Asset",
    }
}
pub(crate) fn apply_known_content_kinds(items: &mut [LibraryItem], known: &BTreeMap<String, String>) {
    for item in items {
        item.ensure_catalog_kind();
        if item.kind == "Project" || item.kind == "Plugin" { continue; }
        if let Some(kind) = known.get(&item.id).filter(|kind| matches!(kind.as_str(), "Project" | "Plugin")) {
            item.kind.clone_from(kind);
        }
    }
}
async fn local_content_kinds() -> BTreeMap<String, String> {
    // One bounded local metadata pass per library operation, not a request or
    // manifest download for every catalogue item. No Epic/Vault state lock.
    tauri::async_runtime::spawn_blocking(crate::vault::known_content_kinds).await.unwrap_or_default()
}
fn merge_content_kinds(known: &mut BTreeMap<String, String>, additional: BTreeMap<String, String>) {
    for (id,kind) in additional {
        if matches!(kind.as_str(),"Project"|"Plugin") && (kind=="Project" || !known.contains_key(&id)) {
            known.insert(id,kind);
        }
    }
}
// Called only after the selected download manifest has been validated, outside
// download_manifest_bound's session guard. It never creates a catalogue entry.
pub(crate) async fn remember_library_kind(state: &EpicState, id: &str, kind: &str) {
    if !matches!(kind,"Project"|"Plugin") { return; }
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let mut session=state.0.lock().await;
    let Some(item)=session.library.iter_mut().find(|item|item.id==id) else { return; };
    item.ensure_catalog_kind();
    if item.kind!="Project" && item.kind!="Plugin" { item.kind=kind.to_owned(); }
    if let Some(account)=session.api.user_details().account_id {
        // Classification caching is best effort and cannot fail a valid plan.
        let _=super::fab_cache::record_manifest_kind(&account,&session.library,id,kind);
    }
}
fn normalize(value: &serde_json::Value) -> Result<LibraryItem, String> {
    let id = value.get("assetId").and_then(|v| v.as_str()).ok_or("Fab вернул запись без assetId")?;
    let title = value.get("title").and_then(|v| v.as_str()).ok_or("Fab вернул запись без названия")?;
    let versions: BTreeSet<String> = value["projectVersions"].as_array().into_iter().flatten()
        .flat_map(|p| p["engineVersions"].as_array().into_iter().flatten())
        .filter_map(|v| v.as_str().map(str::to_string)).collect();
    let categories: Vec<String> = value["categories"].as_array().into_iter().flatten()
        .filter_map(|c| c["name"].as_str().or_else(|| c["id"].as_str()).map(str::to_string)).collect();
    let kind = declared_library_kind(value["distributionMethod"].as_str().unwrap_or(""));
    Ok(LibraryItem { id: id.to_string(), title: title.to_string(), kind: kind.into(), catalog_kind: kind.into(), engine_versions: versions.into_iter().collect(), categories,
        source: value["source"].as_str().unwrap_or("Fab").to_string(),
        namespace: value["assetNamespace"].as_str().unwrap_or("").to_string(),
        description: value["description"].as_str().unwrap_or("").to_string(),
        info_version: LIBRARY_INFO_VERSION,
        images: value["images"].as_array().into_iter().flatten().filter_map(|i|i["url"].as_str().map(str::to_owned)).collect(),
        listing_url: value["url"].as_str().unwrap_or("").to_string(),
        cover: value["images"].as_array().and_then(|images| images.iter().find(|i| i["type"].as_str().is_some_and(|t| t.to_lowercase().contains("thumbnail"))).or_else(|| images.first())).and_then(|i| i["url"].as_str()).map(str::to_string),
        versions: value["projectVersions"].as_array().into_iter().flatten().filter_map(|v| Some(AssetVersion {
            artifact_id: v["artifactId"].as_str()?.to_string(),
            engines: v["engineVersions"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            platforms: v["targetPlatforms"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        })).collect() })
}

#[tauri::command]
pub async fn fab_library(app: tauri::AppHandle, state: tauri::State<'_, EpicState>, on_progress: tauri::ipc::Channel<LibraryProgress>) -> Result<Library, String> {
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let mut known=local_content_kinds().await;
    let mut session = state.0.lock().await;
    if !ensure(&app, &mut session).await? { return Err("Сначала войдите в Epic Games".into()); }
    let details = session.api.user_details();
    let account = details.account_id.as_deref().ok_or("Epic не вернул идентификатор аккаунта")?;
    if !account.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Некорректный идентификатор аккаунта Epic".into()); }
    let token = details.access_token().ok_or("Нет сеанса Epic")?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(40))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("UnrealManager/",env!("CARGO_PKG_VERSION")))
        .build().map_err(|_| "Не удалось создать сетевое соединение")?;
    let endpoint = format!("https://www.fab.com/e/accounts/{account}/ue/library");
    if let Ok(Some(cache))=super::fab_cache::read(account) { merge_content_kinds(&mut known,cache.manifest_kinds); }
    let mut library = fetch_library(&client, &endpoint, token, |pages, items| {
        let _ = on_progress.send(LibraryProgress { pages, items });
    }).await?;
    apply_known_content_kinds(&mut library.items,&known);
    // Cache failure must not discard a successfully synchronized library.
    let _ = super::fab_cache::save(account, &library.items);
    session.library = library.items.clone();
    Ok(library)
}

#[tauri::command]
pub async fn fab_cached_library(app: tauri::AppHandle, state: tauri::State<'_, EpicState>) -> Result<Option<super::fab_cache::CachedLibrary>, String> {
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let known=local_content_kinds().await;
    let mut session=state.0.lock().await;
    if !ensure(&app,&mut session).await? { return Ok(None); }
    let account=session.api.user_details().account_id.ok_or("Нет аккаунта Epic")?;
    let mut cache=super::fab_cache::read(&account)?;
    if let Some(ref mut cache)=cache { apply_known_content_kinds(&mut cache.items,&known); session.library=cache.items.clone(); }
    Ok(cache)
}
#[tauri::command]
pub async fn fab_cover(state: tauri::State<'_, EpicState>, id:String, revision:u64) -> Result<String,String> {
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let url={let session=state.0.lock().await;session.library.iter().find(|i| i.id==id).and_then(|i|i.cover.clone()).ok_or("У материала нет обложки")?};
    super::fab_cache::thumbnail(id, url, revision).await
}

pub(crate) async fn download_manifest(app: &tauri::AppHandle, state: &EpicState, id: &str, artifact: &str)
    -> Result<(LibraryItem, egs_api::api::types::download_manifest::DownloadManifest), String> {
    download_manifest_bound(app,state,id,artifact,None).await
}

// Opaque, native-only queue binding. Never return account/session data to queue IPC.
fn queue_identity(session:&Session)->Result<String,String>{
    use sha2::{Digest,Sha256};
    let account=session.api.user_details().account_id.ok_or("Сначала войдите в Epic")?;
    Ok(format!("{:x}",Sha256::digest(account.as_bytes())))
}
pub(crate) async fn queue_account(app:&tauri::AppHandle,state:&EpicState)->Result<String,String>{
    let mut session=state.0.lock().await;
    load(app,&mut session)?;
    queue_identity(&session)
}
pub(crate) async fn download_manifest_bound(app: &tauri::AppHandle, state: &EpicState, id: &str, artifact: &str, account:Option<&str>)
    -> Result<(LibraryItem, egs_api::api::types::download_manifest::DownloadManifest), String> {
    let mut session = state.0.lock().await;
    load(app,&mut session)?;
    if let Some(expected)=account {if queue_identity(&session)?!=expected{return Err("Аккаунт изменился. Удалите задание и добавьте его заново.".into());}}
    if !ensure(app, &mut session).await? { return Err("Сначала войдите в Epic".into()); }
    let item = session.library.iter().find(|i| i.id == id && i.versions.iter().any(|v| v.artifact_id == artifact))
        .cloned().ok_or("Обновите библиотеку и выберите доступную версию материала")?;
    let infos = tokio::time::timeout(Duration::from_secs(45), session.api.fab_asset_manifest(artifact, &item.namespace, &item.id, Some("Windows")))
        .await.map_err(|_| "Fab не ответил вовремя")?.map_err(|error| api_failure(error, "Fab не предоставил файлы выбранной версии"))?;
    for info in infos {
        for url in &info.distribution_point_base_urls {
            if let Ok(Ok(manifest)) = tokio::time::timeout(Duration::from_secs(45), session.api.fab_download_manifest(info.clone(), url)).await {
                return Ok((item, manifest));
            }
        }
    }
    Err("Не удалось получить описание файлов. Повторите позже".into())
}

#[derive(Clone, Serialize)]
pub struct LibraryProgress { pages: usize, items: usize }

async fn fetch_library(client: &reqwest::Client, endpoint: &str, token: &str, mut progress: impl FnMut(usize, usize)) -> Result<Library, String> {
    let mut cursor: Option<String> = None;
    let mut seen_cursors = BTreeSet::new();
    let mut seen_items = BTreeSet::new();
    let mut items = Vec::new();
    for page_index in 0..1000 {
        let mut request = client.get(endpoint).bearer_auth(token).query(&[("count", "100")]);
        if let Some(ref cursor) = cursor { request = request.query(&[("cursor", cursor)]); }
        let response = request.send().await.map_err(|_| "Не удалось связаться с Fab. Проверьте подключение")?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 => AUTH_REQUIRED.into(),
                403 => "Fab отказал в доступе. Откройте Fab в браузере и проверьте доступ аккаунта; повторите позже".into(),
                429 => "Fab ограничил частоту запросов. Повторите позже".into(),
                _ => format!("Fab вернул ошибку HTTP {}. Библиотека не обновлена", status.as_u16()),
            });
        }
        let page: serde_json::Value = response.json().await.map_err(|_| "Fab вернул неожиданный формат ответа")?;
        let results = page["results"].as_array().ok_or("В ответе Fab отсутствует список библиотеки")?;
        for item in results { let item = normalize(item)?; if seen_items.insert(item.id.clone()) { items.push(item); } }
        let cursors = page["cursors"].as_object().ok_or("Fab не вернул сведения о страницах")?;
        cursor = match cursors.get("next") {
            Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(serde_json::Value::String(_)) | Some(serde_json::Value::Null) => None,
            _ => return Err("Fab вернул некорректный указатель следующей страницы".into()),
        };
        progress(page_index + 1, items.len());
        match &cursor {
            None => { items.sort_by_key(|i| i.title.to_lowercase()); return Ok(Library { items, scope: "unreal-engine" }); }
            Some(c) if !seen_cursors.insert(c.clone()) => return Err("Fab повторил страницу. Частичный список не сохранён".into()),
            _ => (),
        }
    }
    Err("Превышен предел страниц Fab. Частичный список не сохранён".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn website_flow_is_exclusive_and_released_on_error() {
        let attempt=||->Result<(),String>{let _guard=reserve_web_flow()?;Err("synthetic-flow-failure".into())};
        let held=reserve_web_flow().unwrap();
        assert!(attempt().is_err());
        drop(held);
        assert_eq!(attempt().unwrap_err(),"synthetic-flow-failure");
        assert!(reserve_web_flow().is_ok());
    }
    fn mock(responses: Vec<(u16, serde_json::Value)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/library",listener.local_addr().unwrap());
        let handle=std::thread::spawn(move || {
            let mut requests=Vec::new();
            for (status,body) in responses {
                let (mut stream,_)=listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut data=[0u8;4096]; let n=stream.read(&mut data).unwrap();
                requests.push(String::from_utf8_lossy(&data[..n]).to_string());
                let body=body.to_string();
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
            requests
        });
        (url,handle)
    }
    #[tokio::test]
    async fn pagination_and_deduplication() {
        let (url,server)=mock(vec![
            (200,serde_json::json!({"results":[{"assetId":"1","title":"Z"}],"cursors":{"next":"a+b/="}})),
            (200,serde_json::json!({"results":[{"assetId":"1","title":"Z"},{"assetId":"2","title":"A"}],"cursors":{"next":null}})),
        ]);
        let mut updates = Vec::new();
        let result=fetch_library(&reqwest::Client::new(),&url,"synthetic-test-token", |p, n| updates.push((p, n))).await.unwrap();
        assert_eq!(updates, vec![(1, 1), (2, 2)]);
        assert_eq!(result.items.len(),2); assert_eq!(result.items[0].title,"A");
        let requests=server.join().unwrap(); assert!(requests[1].contains("cursor=a%2Bb%2F%3D"));
    }
    #[tokio::test]
    async fn failed_second_page_is_not_partial_success() {
        let (url,server)=mock(vec![
            (200,serde_json::json!({"results":[{"assetId":"1","title":"Z"}],"cursors":{"next":"page2"}})),
            (403,serde_json::json!({"error":"denied"})),
        ]);
        assert!(fetch_library(&reqwest::Client::new(),&url,"synthetic-test-token", |_, _| {}).await.is_err());
        server.join().unwrap();
    }
    #[tokio::test]
    async fn library_401_requires_login_without_exposing_response_or_retrying() {
        let (url,server)=mock(vec![(401,serde_json::json!({"error":"synthetic-private-response"}))]);
        let mut progress=Vec::new();
        let error=fetch_library(&reqwest::Client::new(),&url,"synthetic-private-token",|p,n|progress.push((p,n))).await.err().unwrap();
        assert_eq!(error,AUTH_REQUIRED);assert!(progress.is_empty());
        assert!(!error.contains("synthetic"));assert!(!error.contains(&url));
        assert_eq!(server.join().unwrap().len(),1);
    }
    #[tokio::test]
    async fn library_non_auth_statuses_keep_their_distinct_meaning() {
        for (status,expected) in [(403,"Fab отказал в доступе"),(429,"Fab ограничил частоту"),(500,"HTTP 500")] {
            let (url,server)=mock(vec![(status,serde_json::json!({"error":"synthetic-private-response"}))]);
            let error=fetch_library(&reqwest::Client::new(),&url,"synthetic-private-token",|_,_|{}).await.err().unwrap();
            assert_ne!(error,AUTH_REQUIRED);assert!(error.contains(expected));assert!(!error.contains("synthetic"));
            assert_eq!(server.join().unwrap().len(),1);
        }
    }
    #[test]
    fn api_authentication_requires_a_typed_401_not_message_text_or_forbidden() {
        use egs_api::api::error::EpicAPIError;
        let fallback="Fab не предоставил файлы выбранной версии";
        for status in [401,403,429,500] {
            let error=EpicAPIError::HttpError{status:reqwest::StatusCode::from_u16(status).unwrap(),body:"synthetic-private-response".into()};
            assert_eq!(api_failure(error,fallback),if status==401{AUTH_REQUIRED}else{fallback});
        }
        for error in [EpicAPIError::APIError("401 expired synthetic-private-response".into()),EpicAPIError::InvalidCredentials,EpicAPIError::FabTimeout] {
            assert_eq!(api_failure(error,fallback),fallback);
        }
    }
    #[tokio::test]
    async fn account_exchange_401_requires_login_without_exposing_response() {
        let (url,server)=mock(vec![(401,serde_json::json!({"error":"synthetic-private-response"}))]);
        assert_eq!(request_web_exchange(&exchange_client().unwrap(),&url,"synthetic-private-token").await.unwrap_err(),AUTH_REQUIRED);
        assert_eq!(server.join().unwrap().len(),1);
    }
    #[tokio::test]
    async fn repeated_cursor_stops() {
        let page=serde_json::json!({"results":[],"cursors":{"next":"same"}});
        let (url,server)=mock(vec![(200,page.clone()),(200,page)]);
        assert!(fetch_library(&reqwest::Client::new(),&url,"synthetic-test-token", |_, _| {}).await.is_err());
        server.join().unwrap();
    }
    #[tokio::test]
    async fn web_exchange_uses_one_native_bearer_request() {
        let code="synthetic_Exchange_Code012345";
        let (url,server)=mock(vec![(200,serde_json::json!({"code":code,"expiresInSeconds":30,"creatingClientId":"synthetic-client"}))]);
        let client=exchange_client().unwrap();
        let built=client.get(&url).bearer_auth("synthetic-access-token").build().unwrap();
        assert!(built.headers()[reqwest::header::AUTHORIZATION].is_sensitive());
        assert_eq!(request_web_exchange(&client,&url,"synthetic-access-token").await.unwrap(),code);
        let requests=server.join().unwrap();
        assert_eq!(requests.len(),1);
        assert!(requests[0].starts_with("GET /library HTTP/1.1\r\n"));
        assert!(requests[0].to_ascii_lowercase().contains("authorization: bearer synthetic-access-token\r\n"));
    }
    #[tokio::test]
    async fn web_exchange_rejects_failed_expired_and_oversized_responses_without_echoing() {
        for (status,body) in [
            (403,serde_json::json!({"error":"synthetic-sensitive-response"})),
            (200,serde_json::json!({"code":"synthetic-sensitive-response","expiresInSeconds":0})),
            (200,serde_json::json!({"code":"synthetic-sensitive-response","expiresInSeconds":-1})),
            (200,serde_json::json!({"unexpected":"synthetic-sensitive-response"})),
            (200,serde_json::json!({"code":"synthetic-sensitive-response","expiresInSeconds":30,"padding":"x".repeat(9*1024)})),
        ] {
            let (url,server)=mock(vec![(status,body)]);
            let error=request_web_exchange(&exchange_client().unwrap(),&url,"synthetic-access-token").await.unwrap_err();
            assert_eq!(error,WEB_EXCHANGE_ERROR);
            assert!(!error.contains("synthetic"));assert!(!error.contains(&url));
            server.join().unwrap();
        }
    }
    #[tokio::test]
    async fn web_exchange_bounds_unknown_length_and_malformed_body() {
        for body in [b"synthetic-invalid-json-response".to_vec(),vec![b'x';9*1024]] {
            let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url=format!("http://{}/exchange",listener.local_addr().unwrap());
            let server=std::thread::spawn(move || {
                let (mut stream,_)=listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request=[0u8;4096];stream.read(&mut request).unwrap();
                // No Content-Length: the streaming bound must still apply.
                let _=stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                let _=stream.write_all(&body);
            });
            assert_eq!(request_web_exchange(&exchange_client().unwrap(),&url,"synthetic-access-token").await.unwrap_err(),WEB_EXCHANGE_ERROR);
            server.join().unwrap();
        }
    }
    #[tokio::test]
    async fn web_exchange_never_follows_a_redirect_with_native_credentials() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/exchange",listener.local_addr().unwrap());
        let redirect=format!("http://{}/redirected",listener.local_addr().unwrap());
        let server=std::thread::spawn(move || {
            let (mut stream,_)=listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request=[0u8;4096];stream.read(&mut request).unwrap();
            write!(stream,"HTTP/1.1 302 Found\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            drop(stream);listener.set_nonblocking(true).unwrap();
            let until=std::time::Instant::now()+Duration::from_millis(250);
            while std::time::Instant::now()<until {
                match listener.accept() {
                    Ok((mut stream,_))=>{
                        let _=stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return true;
                    },
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(5)),
                    Err(e)=>panic!("Loopback accept failed: {e}"),
                }
            }
            false
        });
        assert_eq!(request_web_exchange(&exchange_client().unwrap(),&url,"synthetic-access-token").await.unwrap_err(),WEB_EXCHANGE_ERROR);
        assert!(!server.join().unwrap(),"A redirected request was sent");
    }
    #[test] fn code_formats_and_rejections() {
        assert_eq!(parse_code("{\"authorizationCode\":\"abcdefghijklmnop\"}").unwrap(), "abcdefghijklmnop");
        assert!(parse_code("{\"error\":\"no code\"}").is_err());
        assert!(parse_code("https://example.com/token").is_err());
        assert!(parse_code("password").is_err());
    }
    #[test] fn library_mapping_and_validation() {
        let item = normalize(&serde_json::json!({"assetId":"x","title":"Example","distributionMethod":"ENGINE_PLUGIN","categories":null,"projectVersions":[{"engineVersions":["5.8","5.7"]},{"engineVersions":["5.8"]}]})).unwrap();
        assert_eq!(item.kind,"Plugin"); assert_eq!(item.engine_versions,vec!["5.7","5.8"]);
        for method in ["COMPLETE_PROJECT", "CompleteProject", "Complete Project"] {
            let project=normalize(&serde_json::json!({"assetId":"project","title":"Example project","distributionMethod":method})).unwrap();
            assert_eq!(project.kind,"Project");
        }
        assert!(normalize(&serde_json::json!({"error":"denied"})).is_err());
    }
    #[test] fn catalogue_kind_uses_only_exact_distribution_aliases() {
        for method in ["CompleteProject", "COMPLETE_PROJECT", "complete-project", " Complete Project ", "Project"] {
            assert_eq!(declared_library_kind(method), "Project");
        }
        for method in ["ENGINE_PLUGIN", "Engine Plugin", "plugin"] {
            assert_eq!(declared_library_kind(method), "Plugin");
        }
        for method in ["ASSET_PACK", "ADD_TO_PROJECT", "projectile", "plugin collection", "not_a_complete_project", "", "unknown"] {
            assert_eq!(declared_library_kind(method), "Asset");
        }
        for distribution in [serde_json::Value::Null, serde_json::json!({"kind":"CompleteProject"}), serde_json::json!(42)] {
            let item=normalize(&serde_json::json!({"assetId":"fixture","title":"Complete Project Plugin", "description":"Complete Project", "distributionMethod":distribution, "categories":[{"name":"Projects"}]})).unwrap();
            assert_eq!(item.kind, "Asset");
            assert_eq!(item.info_version, LIBRARY_INFO_VERSION);
        }
    }
    #[test] fn known_local_kinds_promote_assets_without_demoting_declared_types() {
        let fixture=|id:&str,method:&str|normalize(&serde_json::json!({"assetId":id,"title":"Synthetic item","distributionMethod":method})).unwrap();
        let mut items=vec![fixture("project","ASSET_PACK"),fixture("plugin","Asset"),fixture("declaredPlugin","ENGINE_PLUGIN"),fixture("declaredProject","COMPLETE_PROJECT"),fixture("unknown","Asset")];
        let known=BTreeMap::from([("project".into(),"Project".into()),("plugin".into(),"Plugin".into()),("declaredPlugin".into(),"Project".into()),("declaredProject".into(),"Plugin".into()),("unknown".into(),"Unexpected".into()),("missing".into(),"Project".into())]);
        apply_known_content_kinds(&mut items,&known);
        assert_eq!(items.iter().map(|item|item.kind.as_str()).collect::<Vec<_>>(),vec!["Project","Plugin","Plugin","Project","Asset"]);
        assert_eq!(items.len(),5);assert!(items.iter().all(|item|item.title=="Synthetic item"));
        assert_eq!(items[0].declared_kind(),"Asset");assert_eq!(items[1].declared_kind(),"Asset");
    }
    #[test] fn known_content_kind_merge_keeps_project_evidence_and_rejects_unknown_types() {
        let mut known=BTreeMap::from([("project".into(),"Project".into()),("plugin".into(),"Plugin".into())]);
        merge_content_kinds(&mut known,BTreeMap::from([("project".into(),"Plugin".into()),("plugin".into(),"Project".into()),("unknown".into(),"Unexpected".into())]));
        assert_eq!(known,BTreeMap::from([("project".into(),"Project".into()),("plugin".into(),"Project".into())]));
    }
    #[tokio::test] async fn remembered_manifest_type_keeps_catalogue_base_and_never_adds_owned_items() {
        let state=EpicState::default();
        let fixture=|id:&str|normalize(&serde_json::json!({"assetId":id,"title":"Synthetic item","distributionMethod":"ASSET_PACK"})).unwrap();
        {let mut session=state.0.lock().await;session.library=vec![fixture("selected"),fixture("other")];}
        remember_library_kind(&state,"selected","Project").await;
        remember_library_kind(&state,"other","Unexpected").await;
        remember_library_kind(&state,"unowned","Plugin").await;
        let session=state.0.lock().await;
        assert_eq!(session.library.len(),2);assert_eq!(session.library[0].kind,"Project");
        assert_eq!(session.library[0].declared_kind(),"Asset");assert_eq!(session.library[1].kind,"Asset");
        assert!(!session.loaded);assert!(!session.remember);
    }
    #[cfg(windows)]
    #[test] fn dpapi_roundtrip_and_corruption() {
        let source=b"synthetic-test-secret";
        let encrypted=protect(source,false).unwrap(); assert_ne!(encrypted,source);
        assert_eq!(protect(&encrypted,true).unwrap(),source);
        assert!(protect(b"invalid",true).is_err());
    }
}




pub(crate) async fn library_snapshot(state:&EpicState)->Vec<LibraryItem>{state.0.lock().await.library.clone()}

// Native-only handoff. Do not derive Debug/Serialize or expose the exchange code
// to frontend IPC, scripts, logs, settings or the saved DPAPI session.
pub(crate) struct WebSession {
    pub remember: bool,
    pub exchange_code: Option<String>,
}

const WEB_EXCHANGE_URL:&str="https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/exchange";
const WEB_EXCHANGE_ERROR:&str="Не удалось открыть окно активации";
fn exchange_client()->Result<reqwest::Client,String>{
    reqwest::Client::builder().timeout(Duration::from_secs(40)).redirect(reqwest::redirect::Policy::none())
        .build().map_err(|_|WEB_EXCHANGE_ERROR.into())
}
async fn request_web_exchange(client:&reqwest::Client,url:&str,access_token:&str)->Result<String,String>{
    // Use bounded, silent parsing here: egs-api's generic parser can log a raw
    // response body on failure. Never include tokens, URLs or bodies in errors.
    let mut response=client.get(url).bearer_auth(access_token).send().await.map_err(|_|WEB_EXCHANGE_ERROR)?;
    const LIMIT:usize=8*1024;
    if response.status()==reqwest::StatusCode::UNAUTHORIZED{return Err(AUTH_REQUIRED.into());}
    if response.status()!=reqwest::StatusCode::OK || response.content_length().is_some_and(|n|n>LIMIT as u64){return Err(WEB_EXCHANGE_ERROR.into());}
    let mut body=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|WEB_EXCHANGE_ERROR)?{
        if body.len()+chunk.len()>LIMIT{return Err(WEB_EXCHANGE_ERROR.into());}body.extend_from_slice(&chunk);
    }
    #[derive(Deserialize)]#[serde(rename_all="camelCase")]
    struct Exchange {code:String,expires_in_seconds:i64}
    let parsed=serde_json::from_slice::<Exchange>(&body).map_err(|_|WEB_EXCHANGE_ERROR);
    body.fill(0);
    let parsed=parsed?;
    if parsed.expires_in_seconds<=0{return Err(WEB_EXCHANGE_ERROR.into());}
    Ok(parsed.code)
}

pub(crate) async fn prepare_web_session(app: &tauri::AppHandle, state: &EpicState) -> Result<WebSession,String> {
    let _update_operation=crate::app_updates::operation()?;
    let mut session=state.0.lock().await;
    let signed_in=ensure(app,&mut session).await?;
    let remember=session.remember;
    if !signed_in { return Ok(WebSession { remember, exchange_code: None }); }
    // A fresh single-use code binds the website to the currently connected
    // account even when website cookies are missing, expired or InPrivate.
    let details=session.api.user_details();
    let access_token=details.access_token().ok_or(WEB_EXCHANGE_ERROR)?;
    let code=request_web_exchange(&exchange_client()?,WEB_EXCHANGE_URL,access_token).await?;
    Ok(WebSession { remember, exchange_code: Some(code) })
}

// Native-only identity binding. Never return account IDs or credentials in a
// redemption snapshot; the displayed name is informational, not authorization.
pub(crate) async fn web_account_binding(app:&tauri::AppHandle,state:&EpicState)->Result<(String,String),String>{
    let mut session=state.0.lock().await;
    if !ensure(app,&mut session).await? {return Err(AUTH_REQUIRED.into());}
    let details=session.api.user_details();
    let id=details.account_id.ok_or(AUTH_REQUIRED)?;
    if id.len()!=32 || !id.bytes().all(|b|b.is_ascii_hexdigit()){return Err(AUTH_REQUIRED.into());}
    Ok((id,details.display_name.unwrap_or_else(||"Epic Games".into())))
}

pub(crate) async fn library_item(state: &EpicState, id: &str) -> Result<LibraryItem,String> {
    state.0.lock().await.library.iter().find(|i|i.id==id).cloned().ok_or("Материал отсутствует в текущей библиотеке".into())
}
pub(crate) async fn library_metadata(state:&EpicState,id:&str)->Result<serde_json::Value,String>{
    let mut session=state.0.lock().await;
    let item=session.library.iter().find(|i|i.id==id).ok_or("Нет материала в библиотеке")?;
    let asset=egs_api::api::types::epic_asset::EpicAsset{catalog_item_id:item.id.clone(),namespace:item.namespace.clone(),..Default::default()};
    let data=tokio::time::timeout(Duration::from_secs(20),session.api.try_asset_info(&asset)).await.map_err(|_|"Каталог Epic не ответил")?.map_err(|_|"Каталог Epic не предоставил подробности")?;
    let data=data.ok_or("Материал отсутствует в каталоге Epic")?;
    if data.id!=asset.catalog_item_id||data.namespace!=asset.namespace{return Err("Не совпадает материал каталога Epic".into());}
    serde_json::to_value(data).map_err(|_|"Не удалось прочитать карточку Epic".into())
}

async fn owned_engine_packages(app: &tauri::AppHandle, state: &EpicState) -> Result<Vec<egs_api::api::types::epic_asset::EpicAsset>, String> {
    let mut session=state.0.lock().await;
    if !ensure(app,&mut session).await? {return Err("Войдите в Epic для доступа к движкам".into());}
    let assets=tokio::time::timeout(Duration::from_secs(45),session.api.try_list_assets(Some("Windows".into()),Some("Live".into()))).await
        .map_err(|_|"Epic не ответил вовремя")?.map_err(|_|"Не удалось получить список движков Epic")?;
    Ok(assets.into_iter().filter(|a|a.namespace=="ue").collect())
}
pub(crate) async fn engine_assets(app:&tauri::AppHandle,state:&EpicState)->Result<Vec<egs_api::api::types::epic_asset::EpicAsset>,String>{
    Ok(owned_engine_packages(app,state).await?.into_iter().filter(|a|a.app_name.strip_prefix("UE_").is_some_and(|s|s.split('.').count()==2&&s.split('.').all(|x|!x.is_empty()&&x.chars().all(|c|c.is_ascii_digit())))).collect())
}


pub(crate) async fn engine_metadata(app: &tauri::AppHandle,state:&EpicState,app_name:&str)->Result<serde_json::Value,String>{
    let asset=engine_assets(app,state).await?.into_iter().find(|a|a.app_name==app_name).ok_or("Движок недоступен в аккаунте Epic")?;
    let mut session=state.0.lock().await;
    let info=tokio::time::timeout(Duration::from_secs(45),session.api.try_asset_info(&asset)).await.map_err(|_|"Epic не ответил")?.map_err(|_|"Нет описания компонентов")?;
    Ok(serde_json::to_value(info).map_err(|_|"Не удалось прочитать компоненты")?)
}
pub(crate) async fn engine_manifest(app:&tauri::AppHandle,state:&EpicState,app_name:&str)->Result<(Vec<u8>,String,Option<String>),String>{
    let asset=owned_engine_packages(app,state).await?.into_iter().find(|a|a.app_name==app_name).ok_or("Пакет недоступен в аккаунте Epic")?;
    let manifest={
        let mut session=state.0.lock().await;
        tokio::time::timeout(Duration::from_secs(45),session.api.try_asset_manifest(Some("Windows".into()),Some("Live".into()),Some(asset.namespace),Some(asset.catalog_item_id.clone()),Some(asset.app_name))).await.map_err(|_|"Epic не ответил")?.map_err(|_|"Не удалось получить манифест движка")?
    };
    let client=reqwest::Client::builder().timeout(Duration::from_secs(120)).build().map_err(|_|"Ошибка сети")?;
    for element in manifest.elements {
        if element.app_name!=app_name {continue;}
        for m in element.manifests {
            if m.uri.scheme()!="https" {continue;}
            let mut url=m.uri.clone(); for p in m.query_params {url.query_pairs_mut().append_pair(&p.name,&p.value);}
            let response=client.get(url).send().await;
            if let Ok(mut response)=response {
                if !response.status().is_success()||response.content_length().is_some_and(|n|n>128*1024*1024){continue;}
                let mut bytes=Vec::new();
                while let Some(chunk)=response.chunk().await.map_err(|_|"Загрузка манифеста прервана")? {
                    if bytes.len()+chunk.len()>128*1024*1024 {return Err("Манифест слишком большой".into());} bytes.extend_from_slice(&chunk);
                }
                let mut base=m.uri.join(".").map_err(|_|"Некорректный адрес манифеста")?;base.set_query(None);base.set_fragment(None);
                return Ok((bytes,base.to_string(),Some(asset.catalog_item_id.clone())));
            }
        }
    }
    Err("Не удалось скачать манифест движка".into())
}


pub(crate) async fn fab_plugin_package(app:&tauri::AppHandle,state:&EpicState,version:&str)->Result<(LibraryItem,egs_api::api::types::download_manifest::DownloadManifest),String>{
 let artifact=format!("FabPlugin_{version}");
 let (raw,base,catalog_id)=engine_manifest(app,state,&artifact).await?;
 let mut manifest=crate::engine::parse(&raw)?;
 manifest.custom_fields.get_or_insert_with(Default::default).insert("SourceURL".into(),base.trim_end_matches('/').into());
 let item=LibraryItem{id:catalog_id.ok_or("Нет идентификатора Fab")?,title:"Fab UE Plugin".into(),kind:"Plugin".into(),catalog_kind:"Plugin".into(),engine_versions:vec![format!("UE_{version}")],categories:vec![],source:"Epic Games Launcher".into(),namespace:"ue".into(),versions:vec![AssetVersion{artifact_id:artifact,engines:vec![format!("UE_{version}")],platforms:vec!["Windows".into()]}],cover:None,description:String::new(),info_version:LIBRARY_INFO_VERSION,images:vec![],listing_url:String::new()};
 Ok((item,manifest))
}

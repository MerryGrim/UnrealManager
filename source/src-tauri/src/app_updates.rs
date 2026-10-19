//! Signed GitHub updates. No background service or recurring polling timer.
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{LazyLock, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};

pub(crate) const REPOSITORY: &str = "MerryGrim/UnrealManager";
pub(crate) const MAX_PAYLOAD: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST: usize = 64 * 1024;
const DAY: u64 = 86400;
static CHECK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static APPLYING: AtomicBool = AtomicBool::new(false);
static OPERATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) struct Operation;
impl Drop for Operation { fn drop(&mut self) { OPERATIONS.fetch_sub(1,Ordering::SeqCst); } }
pub(crate) fn operation() -> Result<Operation,String> {
    if applying() { return Err("Дождитесь завершения обновления программы.".into()); }
    OPERATIONS.fetch_add(1,Ordering::SeqCst);
    let guard=Operation;
    if applying() { drop(guard); return Err("Дождитесь завершения обновления программы.".into()); }
    Ok(guard)
}
pub(crate) fn operations_active() -> bool { OPERATIONS.load(Ordering::SeqCst)>0 }
static OPTIONS: LazyLock<Mutex<Options>> = LazyLock::new(|| Mutex::new(
    read_file(&crate::storage::settings().join("app-updates.json"),4096).ok()
        .and_then(|b| serde_json::from_slice::<Options>(&b).ok()).filter(|p|matches!(p.channel.as_str(),"stable"|"beta")).unwrap_or_default()));

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", default)]
struct Options { enabled: bool, channel: String, last_attempt: u64 }
impl Default for Options {
    fn default() -> Self { Self { enabled: true, channel: if env!("CARGO_PKG_VERSION").contains('-') {"beta"} else {"stable"}.into(), last_attempt: 0 } }
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Settings { enabled: bool, channel: String, configured: bool, current_version: &'static str, mode: String, supported: bool, last_apply_error: Option<String> }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Release { version: String, notes: String, url: String, size: u64, staged: bool }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CheckResult { status: String, release: Option<Release>, checked_at: u64, cached: bool }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct Progress { phase: &'static str, received: u64, total: u64, percent: u8 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub(crate) struct Payload { pub name: String, pub url: String, pub size: u64, pub sha256: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema: u32, pub version: String, pub channel: String, pub target: String,
    pub app_sha256: String, pub installed: Payload, pub portable: Payload,
}
#[derive(Deserialize)] struct PublicKey { algorithm: String, key: String }
#[derive(Serialize, Deserialize)] struct Cached { channel: String, current: String, result: CheckResult }
#[derive(Deserialize)] struct GithubRelease { tag_name: String, draft: bool, prerelease: bool, #[serde(default)] body: Option<String>, assets: Vec<Asset> }
#[derive(Deserialize)] struct Asset { name: String, browser_download_url: String, size: u64, state: String }
#[derive(Serialize, Deserialize)] struct Stage { version: String, directory: String }

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
fn updates() -> PathBuf { crate::storage::root().join("Updates") }
fn persist(options: &Options) -> Result<(), String> { crate::storage::atomic(&crate::storage::settings().join("app-updates.json"), &serde_json::to_vec(options).map_err(err)?) }
fn err(e: impl std::fmt::Display) -> String { format!("Обновление: {e}") }
fn read_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    crate::vault::no_links(path)?;
    let mut bytes=Vec::new();
    fs::File::open(path).map_err(err)?.take(limit as u64+1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len()>limit { return Err("Файл обновления превышает допустимый размер.".into()); }
    Ok(bytes)
}
pub(crate) fn applying() -> bool { APPLYING.load(Ordering::SeqCst) }
pub(crate) fn hash_file(path: &Path) -> Result<String, String> {
    crate::vault::no_links(path)?;
    let mut file = fs::File::open(path).map_err(err)?;
    let mut hash = Sha256::new(); let mut buffer = [0u8; 65536];
    loop { let n = file.read(&mut buffer).map_err(err)?; if n == 0 { break; } hash.update(&buffer[..n]); }
    Ok(format!("{:x}", hash.finalize()))
}
fn key() -> Result<Vec<u8>, String> {
    let public: PublicKey = serde_json::from_str(include_str!("../update-public-key.json")).map_err(err)?;
    if public.algorithm != "ecdsa-p256-sha256-p1363" { return Err("Неподдерживаемый ключ обновлений.".into()); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(public.key).map_err(err)?;
    if bytes.len() != 72 || bytes[..4] != 0x31534345u32.to_le_bytes() || bytes[4..8] != 32u32.to_le_bytes() { return Err("Некорректный ключ обновлений.".into()); }
    Ok(bytes)
}
#[cfg(windows)]
fn verify_signature_with_key(data: &[u8], signature: &[u8], public: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::Security::Cryptography::*;
    if signature.len() != 64 || public.len() != 72 { return Err("Некорректная подпись обновления.".into()); }
    let digest = Sha256::digest(data);
    unsafe {
        let mut algorithm = std::ptr::null_mut();
        if BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_ECDSA_P256_ALGORITHM, std::ptr::null(), 0) < 0 { return Err("Не удалось открыть проверку подписи Windows.".into()); }
        let mut imported = std::ptr::null_mut();
        let status = BCryptImportKeyPair(algorithm, std::ptr::null_mut(), BCRYPT_ECCPUBLIC_BLOB, &mut imported, public.as_ptr(), public.len() as u32, 0);
        let verified = status >= 0 && BCryptVerifySignature(imported, std::ptr::null(), digest.as_ptr(), 32, signature.as_ptr(), 64, 0) >= 0;
        if !imported.is_null() { BCryptDestroyKey(imported); }
        BCryptCloseAlgorithmProvider(algorithm, 0);
        if !verified { return Err("Подпись обновления не прошла проверку.".into()); }
    }
    Ok(())
}
#[cfg(not(windows))]
fn verify_signature_with_key(_: &[u8], _: &[u8], _: &[u8]) -> Result<(), String> { Err("Обновления поддерживаются только в Windows.".into()) }
fn valid_hash(value: &str) -> bool { value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }
fn asset_url(version: &str, name: &str) -> String { format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}") }
fn release_url(version: &str) -> String { format!("https://github.com/{REPOSITORY}/releases/tag/v{version}") }
fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    let version = semver::Version::parse(&manifest.version).map_err(|_| "Некорректная версия обновления.")?;
    if manifest.schema != 1 || manifest.target != "windows-x64" || !valid_hash(&manifest.app_sha256)
        || manifest.channel != if version.pre.is_empty() { "stable" } else { "beta" } { return Err("Некорректный манифест обновления.".into()); }
    for (payload, suffix) in [(&manifest.installed, "Setup.exe"), (&manifest.portable, "Portable.umupdate")] {
        let expected = format!("UnrealManager-{}-Windows-x64-{suffix}", manifest.version);
        if payload.name != expected || payload.url != asset_url(&manifest.version, &expected) || payload.size == 0 || payload.size > MAX_PAYLOAD || !valid_hash(&payload.sha256) { return Err("Некорректный пакет обновления.".into()); }
    }
    Ok(())
}
pub(crate) fn validate_signed_manifest(data: &[u8], signature: &[u8]) -> Result<Manifest, String> {
    if data.is_empty() || data.len() > MAX_MANIFEST || signature.len() > 256 { return Err("Некорректный размер манифеста обновления.".into()); }
    let decoded = base64::engine::general_purpose::STANDARD.decode(std::str::from_utf8(signature).map_err(err)?.trim()).map_err(|_| "Некорректная подпись обновления.")?;
    verify_signature_with_key(data, &decoded, &key()?)?;
    let manifest: Manifest = serde_json::from_slice(data).map_err(|_| "Некорректный манифест обновления.")?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}
pub(crate) fn payload_for<'a>(manifest: &'a Manifest, mode: &str) -> Result<&'a Payload, String> {
    match mode { "installed" => Ok(&manifest.installed), "portable" => Ok(&manifest.portable), _ => Err("Некорректный тип установки.".into()) }
}
fn mode() -> Result<(&'static str, bool), String> {
    let exe = std::env::current_exe().map_err(err)?;
    let folder = exe.parent().ok_or("Не найден каталог программы.")?;
    let development = exe.ancestors().any(|p| p.file_name().is_some_and(|n| n == "target") && p.parent().is_some_and(|p| p.join("Cargo.toml").is_file()));
    Ok((if folder.join("portable.marker").is_file() {"portable"} else {"installed"}, cfg!(windows) && !development))
}
fn last_apply_error() -> Option<String> {
    let staged: Stage = serde_json::from_slice(&read_file(&updates().join("staged.json"),4096).ok()?).ok()?;
    if uuid::Uuid::parse_str(&staged.directory).is_err() { return None; }
    let result = updates().join(staged.directory).join("result.json");
    crate::vault::no_links(&result).ok()?;
    let mut bytes=Vec::new(); fs::File::open(result).ok()?.take(32769).read_to_end(&mut bytes).ok()?;
    if bytes.len()>32768 { return None; }
    let result: serde_json::Value=serde_json::from_slice(&bytes).ok()?;
    if result["ok"].as_bool()!=Some(false) { return None; }
    result["error"].as_str().map(|s|s.chars().take(4000).collect())
}
#[tauri::command]
pub fn app_update_settings_get() -> Settings {
    let p = OPTIONS.lock().unwrap_or_else(|e| e.into_inner());
    let (mode, supported) = mode().unwrap_or(("installed", false));
    Settings { enabled:p.enabled, channel:p.channel.clone(), configured:key().is_ok(), current_version:env!("CARGO_PKG_VERSION"), mode:mode.into(), supported, last_apply_error:last_apply_error() }
}
#[tauri::command]
pub fn app_update_settings_set(enabled: bool, channel: String) -> Result<(), String> {
    if !matches!(channel.as_str(), "stable" | "beta") { return Err("Некорректный канал обновлений.".into()); }
    let mut p = OPTIONS.lock().map_err(err)?;
    let next = Options { enabled, channel:channel.clone(), last_attempt: if channel == p.channel {p.last_attempt} else {0} };
    persist(&next)?; *p = next; Ok(())
}
fn client(redirects: bool) -> Result<reqwest::Client, String> {
    let redirect = if redirects { reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        if attempt.previous().len() >= 5 || url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
            || !matches!(url.host_str(), Some("github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com")) { attempt.stop() } else { attempt.follow() }
    }) } else { reqwest::redirect::Policy::none() };
    reqwest::Client::builder().user_agent(concat!("UnrealManager/",env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(if redirects {300} else {20})).redirect(redirect).build().map_err(err)
}
async fn read_response(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() { return Err("GitHub временно недоступен или пакет не опубликован.".into()); }
    if response.content_length().is_some_and(|n| n > limit as u64) { return Err("Ответ обновления слишком большой.".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "Не удалось прочитать ответ GitHub.")? {
        if bytes.len().saturating_add(chunk.len()) > limit { return Err("Ответ обновления слишком большой.".into()); }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn get_asset(version: &str, name: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = client(true)?.get(asset_url(version,name)).send().await.map_err(|_| "Не удалось загрузить сведения об обновлении.")?;
    read_response(response,limit).await
}
fn candidate(releases: Vec<GithubRelease>, current: &str, channel: &str) -> Result<Option<GithubRelease>, String> {
    let current = semver::Version::parse(current).map_err(err)?;
    Ok(releases.into_iter().filter(|r| {
        let Some(tag) = r.tag_name.strip_prefix('v') else { return false; };
        let Ok(version) = semver::Version::parse(tag) else { return false; };
        if r.draft || version <= current || (channel == "stable" && (r.prerelease || !version.pre.is_empty())) { return false; }
        ["update-manifest.json", "update-manifest.sig"].into_iter().all(|name| r.assets.iter().any(|a| a.name == name && a.state == "uploaded" && a.size > 0 && a.browser_download_url == asset_url(tag,name)))
    }).max_by_key(|r| semver::Version::parse(r.tag_name.trim_start_matches('v')).unwrap()))
}
async fn query(channel: &str) -> Result<Option<(Release,Vec<u8>,Vec<u8>,Manifest)>, String> {
    let response = client(false)?.get(format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100"))
        .header("Accept","application/vnd.github+json").header("X-GitHub-Api-Version","2022-11-28").send().await.map_err(|_| "Не удалось проверить обновления. Проверьте подключение к интернету.")?;
    let bytes = read_response(response,2*1024*1024).await?;
    let releases: Vec<GithubRelease> = serde_json::from_slice(&bytes).map_err(|_| "Некорректный список релизов GitHub.")?;
    let Some(release) = candidate(releases,env!("CARGO_PKG_VERSION"),channel)? else { return Ok(None); };
    let version = release.tag_name.strip_prefix('v').unwrap();
    let bytes = get_asset(version,"update-manifest.json",MAX_MANIFEST).await?;
    let signature = get_asset(version,"update-manifest.sig",256).await?;
    let manifest = validate_signed_manifest(&bytes,&signature)?;
    if manifest.version != version || (channel == "stable" && manifest.channel != "stable") { return Err("Версия или канал пакета не совпадают с релизом.".into()); }
    let (mode,_) = mode()?;
    let payload = payload_for(&manifest,mode)?;
    let staged = staged_directory(version).is_ok();
    Ok(Some((Release { version:version.into(), notes:release.body.unwrap_or_default().chars().take(12000).collect(), url:release_url(version), size:payload.size, staged },bytes,signature,manifest)))
}
fn staged_directory(version: &str) -> Result<PathBuf, String> {
    let staged: Stage = serde_json::from_slice(&read_file(&updates().join("staged.json"),4096).map_err(err)?).map_err(err)?;
    if staged.version != version || uuid::Uuid::parse_str(&staged.directory).is_err() { return Err("Сначала скачайте обновление.".into()); }
    let directory = updates().join(staged.directory);
    crate::vault::no_links(&directory)?;
    let result=directory.join("result.json");
    if result.exists() {
        let outcome:serde_json::Value=serde_json::from_slice(&read_file(&result,32768)?).map_err(err)?;
        if outcome["ok"].as_bool()==Some(false) { return Err("Предыдущее применение не завершилось. Скачайте пакет заново.".into()); }
    }
    let manifest = validate_signed_manifest(&read_file(&directory.join("manifest.json"),MAX_MANIFEST).map_err(err)?, &read_file(&directory.join("manifest.sig"),256).map_err(err)?)?;
    let payload = payload_for(&manifest,mode()?.0)?;
    let file = directory.join(&payload.name);
    if manifest.version != version || fs::metadata(&file).map_err(err)?.len() != payload.size || hash_file(&file)? != payload.sha256 { return Err("Скачанный пакет повреждён. Скачайте его заново.".into()); }
    Ok(directory)
}
fn empty(status: &str) -> CheckResult { CheckResult { status:status.into(),release:None,checked_at:0,cached:false } }
#[tauri::command]
pub async fn app_update_check(manual: bool) -> Result<CheckResult, String> {
    let _guard = CHECK.try_lock().map_err(|_| "Проверка или загрузка обновления уже выполняется.")?;
    if !mode()?.1 { return Ok(empty("disabled")); }
    let options = OPTIONS.lock().map_err(err)?.clone();
    if !manual && !options.enabled { return Ok(empty("disabled")); }
    let timestamp = now();
    if !manual && timestamp.saturating_sub(options.last_attempt) < DAY {
        let cached = read_file(&crate::storage::cache().join("app-updates.json"),65536).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok());
        if let Some(mut cached) = cached.filter(|c| c.channel == options.channel && c.current == env!("CARGO_PKG_VERSION")) {
            cached.result.cached = true;
            if let Some(release) = &mut cached.result.release { release.staged = staged_directory(&release.version).is_ok(); }
            return Ok(cached.result);
        }
        return Ok(empty("deferred"));
    }
    { let mut options = OPTIONS.lock().map_err(err)?; options.last_attempt = timestamp; persist(&options)?; }
    let release = query(&options.channel).await?.map(|q|q.0);
    if OPTIONS.lock().map_err(err)?.channel != options.channel { return Ok(empty("deferred")); }
    let result = CheckResult { status:if release.is_some() {"available"} else {"current"}.into(), release, checked_at:timestamp, cached:false };
    crate::storage::atomic(&crate::storage::cache().join("app-updates.json"), &serde_json::to_vec(&Cached {channel:options.channel,current:env!("CARGO_PKG_VERSION").into(),result:result.clone()}).map_err(err)?)?;
    Ok(result)
}
#[tauri::command]
pub async fn app_update_download(version: String, on_progress: tauri::ipc::Channel<Progress>) -> Result<Release, String> {
    let _guard = CHECK.try_lock().map_err(|_| "Проверка или загрузка обновления уже выполняется.")?;
    if !mode()?.1 { return Err("Обновляйте сборку разработки из исходников.".into()); }
    let options = OPTIONS.lock().map_err(err)?.clone();
    let Some((mut release,manifest_bytes,signature,manifest)) = query(&options.channel).await? else { return Err("Новая версия не найдена.".into()); };
    if release.version != version { return Err("Список обновлений изменился. Проверьте версии снова.".into()); }
    let payload = payload_for(&manifest,mode()?.0)?;
    let id = uuid::Uuid::new_v4().to_string(); let directory = updates().join(&id);
    crate::vault::no_links(&directory)?; fs::create_dir_all(&directory).map_err(err)?;
    fs::write(directory.join("manifest.json"),&manifest_bytes).map_err(err)?;
    fs::write(directory.join("manifest.sig"),&signature).map_err(err)?;
    let result = async {
        let mut response = client(true)?.get(&payload.url).send().await.map_err(|_| "Не удалось скачать обновление.")?;
        if !response.status().is_success() || response.content_length().is_some_and(|s| s != payload.size) { return Err("Размер или статус пакета не совпадает с манифестом.".into()); }
        let part = directory.join("download.part");
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&part).map_err(err)?;
        let mut hash = Sha256::new(); let mut received = 0u64; let mut percent = 255u8;
        while let Some(chunk) = response.chunk().await.map_err(|_| "Загрузка обновления прервана. Повторите попытку.")? {
            received = received.checked_add(chunk.len() as u64).ok_or("Размер пакета слишком большой.")?;
            if received > payload.size { return Err("Размер пакета превышает манифест.".into()); }
            file.write_all(&chunk).map_err(err)?; hash.update(&chunk);
            let next = ((received * 100) / payload.size) as u8;
            if next != percent { percent = next; let _ = on_progress.send(Progress {phase:"downloading",received,total:payload.size,percent}); }
        }
        file.sync_all().map_err(err)?; drop(file);
        let _ = on_progress.send(Progress {phase:"verifying",received,total:payload.size,percent:100});
        if received != payload.size || format!("{:x}",hash.finalize()) != payload.sha256 { return Err("Пакет повреждён или не прошёл проверку SHA-256.".into()); }
        fs::rename(part,directory.join(&payload.name)).map_err(err)?;
        crate::storage::atomic(&updates().join("staged.json"), &serde_json::to_vec(&Stage {version:version.clone(),directory:id}).map_err(err)?)?;
        release.staged = true;
        Ok::<_,String>(release)
    }.await;
    if result.is_err() { let _ = fs::remove_dir_all(&directory); }
    result
}
#[tauri::command]
pub async fn app_update_apply(app: tauri::AppHandle, version: String) -> Result<(), String> {
    let _guard = CHECK.try_lock().map_err(|_| "Дождитесь завершения проверки или загрузки.")?;
    if !mode()?.1 { return Err("Обновляйте сборку разработки из исходников.".into()); }
    if semver::Version::parse(&version).map_err(err)? <= semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(err)? { return Err("Установка прежней версии запрещена.".into()); }
    let directory = staged_directory(&version)?;
    let manifest = validate_signed_manifest(&read_file(&directory.join("manifest.json"),MAX_MANIFEST).map_err(err)?,&read_file(&directory.join("manifest.sig"),256).map_err(err)?)?;
    let channel = OPTIONS.lock().map_err(err)?.channel.clone();
    if channel == "stable" && manifest.channel != "stable" { return Err("Пакет не соответствует выбранному каналу.".into()); }
    if APPLYING.swap(true,Ordering::SeqCst) { return Err("Обновление уже устанавливается.".into()); }
    let result = async {
        crate::window_behavior::prepare_update(&app).await?;
        let original = std::env::current_exe().map_err(err)?.canonicalize().map_err(err)?;
        let (created,_) = crate::app_update_apply::process_identity(std::process::id())?;
        let id = directory.file_name().and_then(|s|s.to_str()).ok_or("Не найден каталог обновления.")?.to_string();
        let job = crate::app_update_apply::Job { schema:1,id,parent_pid:std::process::id(),parent_created:created,original_exe:original.clone(),mode:mode()?.0.into(),manifest:directory.join("manifest.json"),signature:directory.join("manifest.sig"),payload:directory.join(&payload_for(&manifest,mode()?.0)?.name) };
        for name in ["ready.json", "healthy.json", "result.json"] {
            let file = directory.join(name); crate::vault::no_links(&file)?;
            if file.exists() { fs::remove_file(&file).map_err(err)?; }
        }
        let helper = directory.join("unreal-manager.exe"); crate::vault::no_links(&helper)?;
        fs::copy(&original,&helper).map_err(err)?;
        let job_path = directory.join("job.json");
        crate::storage::atomic(&job_path,&serde_json::to_vec(&job).map_err(err)?)?;
        let mut command = std::process::Command::new(&helper);
        command.arg("--apply-app-update").arg(&job_path).current_dir(&directory);
        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let mut child = command.spawn().map_err(err)?;
        let deadline = tokio::time::Instant::now()+Duration::from_secs(10);
        loop {
            if let Ok(bytes) = read_file(&directory.join("ready.json"),4096) {
                if serde_json::from_slice::<serde_json::Value>(&bytes).ok().is_some_and(|v|v["id"]==job.id && v["helperPid"].as_u64()==Some(child.id() as u64)) { app.exit(0); return Ok(()); }
            }
            if child.try_wait().map_err(err)?.is_some() { return Err("Помощник обновления не смог запуститься. Программа остаётся открытой.".into()); }
            if tokio::time::Instant::now() >= deadline { let _=child.kill(); let _=child.wait(); return Err("Помощник обновления не ответил вовремя.".into()); }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }.await;
    if result.is_err() { APPLYING.store(false,Ordering::SeqCst); crate::window_behavior::abort_update(); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        let version="0.2.0-beta.8";
        let payload=|suffix:&str| { let name=format!("UnrealManager-{version}-Windows-x64-{suffix}"); Payload{name:name.clone(),url:asset_url(version,&name),size:123,sha256:"a".repeat(64)} };
        Manifest{schema:1,version:version.into(),channel:"beta".into(),target:"windows-x64".into(),app_sha256:"b".repeat(64),installed:payload("Setup.exe"),portable:payload("Portable.umupdate")}
    }
    #[test] fn signed_manifest_shape_rejects_foreign_targets_urls_and_channels() {
        let valid=manifest();assert!(validate_manifest(&valid).is_ok());
        for changed in 0..7 {let mut m=valid.clone();match changed {0=>m.installed.url="https://example.com/setup.exe".into(),1=>m.portable.name="../app.exe".into(),2=>m.portable.size=MAX_PAYLOAD+1,3=>m.app_sha256="z".repeat(64),4=>m.channel="stable".into(),5=>m.target="linux".into(),_=>m.schema=2};assert!(validate_manifest(&m).is_err());}
        assert!(validate_signed_manifest(&vec![0;MAX_MANIFEST+1],b"bad").is_err());
        assert!(validate_signed_manifest(b"{}",b"not-a-signature").is_err());
    }
    #[test] fn semantic_channels_ignore_unsigned_old_assets_drafts_and_foreign_links() {
        let release=|v:&str| GithubRelease{tag_name:format!("v{v}"),draft:false,prerelease:v.contains('-'),body:None,assets:["update-manifest.json","update-manifest.sig"].iter().map(|n|Asset{name:(*n).into(),browser_download_url:asset_url(v,n),size:123,state:"uploaded".into()}).collect()};
        let all=||vec![release("0.2.0-beta.10"),release("0.2.0-beta.8"),release("0.2.0"),release("0.3.0-beta.1")];
        assert_eq!(candidate(all(),"0.2.0-beta.7","beta").unwrap().unwrap().tag_name,"v0.3.0-beta.1");
        assert_eq!(candidate(all(),"0.2.0-beta.7","stable").unwrap().unwrap().tag_name,"v0.2.0");
        let mut draft=release("1.0.0");draft.draft=true;let mut foreign=release("2.0.0");foreign.assets[0].browser_download_url="https://evil.invalid/file".into();
        assert!(candidate(vec![draft,foreign],"0.2.0-beta.7","beta").unwrap().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn cng_signature_accepts_p1363_and_rejects_modified_bytes_wrong_key_and_truncation() {
        use windows_sys::Win32::Security::Cryptography::*;
        unsafe fn signed(data: &[u8]) -> (Vec<u8>,Vec<u8>) {
            let mut algorithm=std::ptr::null_mut(); let mut key=std::ptr::null_mut();
            assert!(BCryptOpenAlgorithmProvider(&mut algorithm,BCRYPT_ECDSA_P256_ALGORITHM,std::ptr::null(),0)>=0);
            assert!(BCryptGenerateKeyPair(algorithm,&mut key,256,0)>=0);
            assert!(BCryptFinalizeKeyPair(key,0)>=0);
            let mut public=vec![0u8;72];let mut count=0;
            assert!(BCryptExportKey(key,std::ptr::null_mut(),BCRYPT_ECCPUBLIC_BLOB,public.as_mut_ptr(),72,&mut count,0)>=0);assert_eq!(count,72);
            let digest=Sha256::digest(data);let mut signature=vec![0u8;64];
            assert!(BCryptSignHash(key,std::ptr::null(),digest.as_ptr(),32,signature.as_mut_ptr(),64,&mut count,0)>=0);assert_eq!(count,64);
            BCryptDestroyKey(key);BCryptCloseAlgorithmProvider(algorithm,0);
            (public,signature)
        }
        let data=b"{\"version\":\"0.2.0-beta.8\",\"signed\":true}";
        let (public,signature)=unsafe{signed(data)};
        assert!(verify_signature_with_key(data,&signature,&public).is_ok());
        assert!(verify_signature_with_key(b"modified",&signature,&public).is_err());
        let mut changed=signature.clone();changed[0]^=1;
        assert!(verify_signature_with_key(data,&changed,&public).is_err());
        assert!(verify_signature_with_key(data,&signature[..63],&public).is_err());
        let (other,_)=unsafe{signed(data)};
        assert!(verify_signature_with_key(data,&signature,&other).is_err());
        assert!(key().is_ok());
    }
    #[test]
    fn local_metadata_is_bounded_before_parsing() {
        let path=std::env::temp_dir().join(format!("um-update-metadata-{}",uuid::Uuid::new_v4()));
        fs::write(&path,vec![b'x';MAX_MANIFEST+1]).unwrap();
        assert!(read_file(&path,MAX_MANIFEST).is_err());
        fs::write(&path,b"{}").unwrap();
        assert_eq!(read_file(&path,2).unwrap(),b"{}");
        assert!(read_file(&path,1).is_err());
        fs::remove_file(path).unwrap();
    }
}

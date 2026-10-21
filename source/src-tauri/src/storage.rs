use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, sync::{LazyLock, Mutex}};
use serde::Serialize;

pub static CACHE_ACCESS: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    #[cfg(test)] { return std::env::temp_dir().join(format!("um-storage-tests-{}",std::process::id())); }
    #[cfg(not(test))] {
        let exe=std::env::current_exe().expect("Application location unavailable");
        select_root(&exe, &dirs::data_local_dir().expect("Local data directory unavailable"))
    }
});
fn development_root(exe:&Path)->Option<PathBuf> {
    let tauri=exe.parent()?.parent()?.parent()?;
    if tauri.file_name()?.to_str()?=="src-tauri" && tauri.join("Cargo.toml").is_file() {
        Some(tauri.parent()?.parent()?.to_path_buf())
    } else { None }
}
fn select_root(exe:&Path, local:&Path)->PathBuf {
    let folder=exe.parent().expect("Application folder unavailable");
    if folder.join("portable.marker").is_file() { folder.join("Data") }
    else if let Some(dev)=development_root(exe) {dev.join("Data")}
    else {local.join("UnrealManager/Data")}
}
pub fn root()->&'static Path { &ROOT }
pub fn cache()->PathBuf {root().join("Cache")}
pub fn settings()->PathBuf {root().join("Settings")}
pub fn vault()->PathBuf {
    fs::read(settings().join("storage.json")).ok()
        .and_then(|b|serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v|v["vaultPath"].as_str().map(PathBuf::from))
        .filter(|p|p.is_absolute()).unwrap_or_else(||root().join("Vault"))
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct VaultStorage { vault_path: String, keep_engine_backup: bool }
fn storage_options()->serde_json::Value {
    fs::read(settings().join("storage.json")).ok()
        .and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_else(||serde_json::json!({}))
}
pub fn keep_engine_backup()->bool { storage_options()["keepEngineBackup"].as_bool().unwrap_or(false) }
#[tauri::command]
pub fn vault_storage_get()->VaultStorage {
    VaultStorage { vault_path:vault().to_string_lossy().into_owned(),keep_engine_backup:keep_engine_backup() }
}
#[tauri::command]
pub fn vault_storage_set(path:String,keep_engine_backup:bool,state:tauri::State<'_,crate::vault::VaultState>,engine:tauri::State<'_,crate::engine::EngineState>)->Result<(),String> {
    if engine.active.swap(true,std::sync::atomic::Ordering::SeqCst) {return Err("Дождитесь текущей операции с движком.".into());}
    let _engine=crate::engine::install::Busy(&engine.active);
    if state.active.swap(true,std::sync::atomic::Ordering::SeqCst) {return Err("Дождитесь текущей операции Vault".into());}
    let _vault=crate::engine::install::Busy(&state.active);
    let _guard=SETTINGS_LOCK.lock().map_err(error)?;
    let root=validate_vault_path(&path)?;
    fs::create_dir_all(&root).map_err(error)?;
    let mut options=storage_options();
    options["vaultPath"]=root.to_string_lossy().into_owned().into();
    options["keepEngineBackup"]=keep_engine_backup.into();
    atomic(&settings().join("storage.json"),&serde_json::to_vec(&options).map_err(error)?)?;
    crate::vault::discard_plan(&state);
    Ok(())
}
fn validate_vault_path(path:&str)->Result<PathBuf,String> {
    let root=PathBuf::from(path.trim());
    if !root.is_absolute() || root.parent().is_none() || root.file_name().is_none() || root.components().any(|c|matches!(c,std::path::Component::ParentDir)) {
        return Err("Укажите отдельную папку хранилища полным путём.".into());
    }
    crate::vault::no_links(&root)?;
    if root.exists() && !root.is_dir() {return Err("Укажите папку, а не файл.".into());}
    if root.join("Engine/Build/Build.version").exists() || root==settings() || root==cache() || root==self::root() {
        return Err("Выберите отдельную папку для хранилища.".into());
    }
    Ok(root)
}
fn error(e:impl std::fmt::Display)->String {format!("Storage: {e}")}
pub fn atomic(path:&Path, bytes:&[u8])->Result<(),String> {
    crate::vault::no_links(path)?;
    fs::create_dir_all(path.parent().ok_or("Storage folder unavailable")?).map_err(error)?;
    let tmp=path.with_extension(format!("{}.tmp",uuid::Uuid::new_v4()));
    fs::write(&tmp,bytes).map_err(error)?;
    fs::rename(&tmp,path).map_err(error)
}
// Copy once, never move or overwrite old user data. Reparse points are not followed.
fn copy_missing(source:&Path,destination:&Path)->Result<(),String> {
    if !source.exists() || destination.exists() {return Ok(());}
    crate::vault::no_links(source)?;crate::vault::no_links(destination)?;
    if source.is_file() {
        fs::create_dir_all(destination.parent().ok_or("Storage folder unavailable")?).map_err(error)?;
        fs::copy(source,destination).map_err(error)?;
    } else {
        let staging=destination.with_extension(format!("{}.migration",uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging).map_err(error)?;
        for entry in walkdir::WalkDir::new(source).min_depth(1).follow_links(false) {
            let entry=entry.map_err(error)?;crate::vault::no_links(entry.path())?;
            let target=staging.join(entry.path().strip_prefix(source).map_err(error)?);
            if entry.file_type().is_dir() {fs::create_dir_all(target).map_err(error)?;}
            else {fs::copy(entry.path(),target).map_err(error)?;}
        }
        fs::rename(staging,destination).map_err(error)?;
    }
    Ok(())
}
pub fn initialize()->Result<(),String> {
    crate::vault::no_links(root())?;
    fs::create_dir_all(settings()).map_err(error)?;
    // Migration applies only to the existing development checkout, never to a distributed portable build.
    if let Some(dev)=std::env::current_exe().ok().as_deref().and_then(development_root) {
        let marker=settings().join("legacy-import.complete");
        if !marker.exists() {
            copy_missing(&dev.join("Cache"),&cache())?;
            if dev.join("Vault").is_dir() && !settings().join("storage.json").exists() {
                atomic(&settings().join("storage.json"),serde_json::json!({"vaultPath":dev.join("Vault")}).to_string().as_bytes())?;
            }
            copy_missing(&dev.join("Vault/epic-vault.json"),&settings().join("epic-vault.json"))?;
            if let Some(local)=dirs::data_local_dir() {
                let old=local.join("ru.unrealmanager.app");
                copy_missing(&old.join("epic-session.dpapi"),&root().join("Account/epic-session.dpapi"))?;
                copy_missing(&old.join("epic-web"),&root().join("Account/epic-web"))?;
                copy_missing(&old.join("EBWebView"),&root().join("WebView/EBWebView"))?;
                copy_missing(&local.join("UnrealManager/engine-installs.json"),&settings().join("engine-installs.json"))?;
            }
            atomic(&marker,b"1")?;
        }
    }
    fs::create_dir_all(cache()).map_err(error)?;
    Ok(())
}
const KEYS:[&str;5]=["unreal-manager.language.v1","um-library-page-size","um-plugins-page-size","unreal-manager.download-history.v1","um-library-sort"];
fn read_preferences()->Result<BTreeMap<String,String>,String> {
    let path=settings().join("preferences.json");
    if !path.exists(){return Ok(BTreeMap::new());}
    let bytes=fs::read(path).map_err(error)?;
    serde_json::from_slice(&bytes).map_err(error)
}
#[tauri::command]
pub fn settings_read()->Result<BTreeMap<String,String>,String>{
    let _guard=SETTINGS_LOCK.lock().map_err(error)?;read_preferences()
}
#[tauri::command]
pub fn settings_write(key:String,value:String)->Result<(),String>{
    if !KEYS.contains(&key.as_str()) || value.len()>8*1024*1024 {return Err("Invalid preference".into());}
    let _guard=SETTINGS_LOCK.lock().map_err(error)?;
    let mut values=read_preferences()?;values.insert(key,value);
    atomic(&settings().join("preferences.json"),&serde_json::to_vec_pretty(&values).map_err(error)?)
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct StorageInfo {data_path:String,cache_path:String,settings_path:String,cache_bytes:u64}
fn cache_files(base:&Path)->Result<Vec<PathBuf>,String> {
    crate::vault::no_links(base)?;
    let mut paths=Vec::new();
    for name in ["libraries","covers","gallery","details","engines.json"] {
        let path=base.join(name);
        if !path.exists(){continue;}
        for entry in walkdir::WalkDir::new(&path).follow_links(false) {
            let entry=entry.map_err(error)?;crate::vault::no_links(entry.path())?;
        }
        paths.push(path);
    }
    Ok(paths)
}
fn size(paths:&[PathBuf])->Result<u64,String> {
    let mut bytes=0;
    for path in paths {for entry in walkdir::WalkDir::new(path){let e=entry.map_err(error)?;if e.file_type().is_file(){bytes+=e.metadata().map_err(error)?.len();}}}
    Ok(bytes)
}
fn clear_at(base:&Path)->Result<u64,String> {
    let paths=cache_files(base)?;let bytes=size(&paths)?;
    for path in paths {if path.is_dir(){fs::remove_dir_all(path).map_err(error)?;}else{fs::remove_file(path).map_err(error)?;}}
    Ok(bytes)
}
#[tauri::command]
pub async fn storage_info()->Result<StorageInfo,String> {
    let _guard=CACHE_ACCESS.read().await;
    tauri::async_runtime::spawn_blocking(||Ok(StorageInfo{data_path:root().display().to_string(),cache_path:cache().display().to_string(),settings_path:settings().display().to_string(),cache_bytes:size(&cache_files(&cache())?)?})).await.map_err(error)?
}
#[tauri::command]
pub async fn clear_cache()->Result<u64,String> {
    let _guard=CACHE_ACCESS.write().await;
    tauri::async_runtime::spawn_blocking(||clear_at(&cache())).await.map_err(error)?
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn vault_location_rejects_drive_root_relative_file_and_engine(){
     assert!(validate_vault_path("relative/vault").is_err());assert!(validate_vault_path("X:\\").is_err());assert!(validate_vault_path("X:\\folder\\..\\vault").is_err());
     let dir=std::env::temp_dir().join(format!("um-vault-path-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(dir.join("Engine/Build")).unwrap();fs::write(dir.join("Engine/Build/Build.version"),b"{}").unwrap();assert!(validate_vault_path(dir.to_str().unwrap()).is_err());assert!(validate_vault_path(dir.join("Vault").to_str().unwrap()).is_ok());fs::write(dir.join("file"),b"data").unwrap();assert!(validate_vault_path(dir.join("file").to_str().unwrap()).is_err());fs::remove_dir_all(dir).unwrap();
    }

    #[test] fn migration_keeps_source_and_never_overwrites_destination(){
        let dir=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(dir.join("old/nested")).unwrap();fs::write(dir.join("old/nested/settings"),b"old").unwrap();
        copy_missing(&dir.join("old"),&dir.join("new")).unwrap();
        fs::write(dir.join("new/nested/settings"),b"new").unwrap();
        copy_missing(&dir.join("old"),&dir.join("new")).unwrap();
        assert_eq!(fs::read(dir.join("old/nested/settings")).unwrap(),b"old");
        assert_eq!(fs::read(dir.join("new/nested/settings")).unwrap(),b"new");fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn preference_command_rejects_paths_and_oversized_values(){
        assert!(settings_write("../secret".into(),"x".into()).is_err());
        assert!(settings_write(KEYS[0].into(),"x".repeat(8*1024*1024+1)).is_err());
    }
    #[test] fn portable_and_installed_paths(){
        let dir=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());fs::create_dir_all(&dir).unwrap();
        let exe=dir.join("um.exe");assert_eq!(select_root(&exe,&dir.join("profile")),dir.join("profile/UnrealManager/Data"));
        fs::write(dir.join("portable.marker"),b"").unwrap();assert_eq!(select_root(&exe,&dir.join("profile")),dir.join("Data"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn cache_clear_preserves_settings_vault_and_unknown_files(){
        let dir=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(dir.join("Cache/covers")).unwrap();fs::create_dir_all(dir.join("Settings")).unwrap();fs::create_dir_all(dir.join("Vault")).unwrap();
        fs::write(dir.join("Cache/covers/a.jpg"),b"cover").unwrap();fs::write(dir.join("Cache/engines.json"),b"[]").unwrap();
        for p in ["Settings/preferences.json","Vault/asset","Cache/unknown.txt"]{fs::write(dir.join(p),b"keep").unwrap();}
        assert_eq!(clear_at(&dir.join("Cache")).unwrap(),7);assert_eq!(clear_at(&dir.join("Cache")).unwrap(),0);
        for p in ["Settings/preferences.json","Vault/asset","Cache/unknown.txt"]{assert_eq!(fs::read(dir.join(p)).unwrap(),b"keep");}
        fs::remove_dir_all(dir).unwrap();
    }
}

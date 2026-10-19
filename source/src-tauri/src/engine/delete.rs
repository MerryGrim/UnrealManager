use super::*;

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct DeletePreview { path:String, version:String }

fn checked_root(path:&str, known:&[PathBuf]) -> Result<PathBuf,String> {
    let root=target(path)?;
    let canonical=root.canonicalize().map_err(io)?;
    if !known.iter().filter_map(|p|p.canonicalize().ok()).any(|p|p==canonical) {
        return Err("Папка не соответствует установленному движку. Обновите список.".into());
    }
    if root.join(".um-engine-update.json").exists() { return Err("Сначала завершите или восстановите обновление движка.".into()); }
    version(&root)?;
    if !root.join("Engine/Binaries/Win64/UnrealEditor.exe").is_file() { return Err("Не найден редактор Unreal Engine.".into()); }
    Ok(root)
}
fn known_roots()->Vec<PathBuf>{crate::scan_engines().into_iter().map(|e|PathBuf::from(e.path)).collect()}

#[tauri::command]
pub async fn engine_delete_preview(path:String)->Result<DeletePreview,String>{
    tauri::async_runtime::spawn_blocking(move||{
        let root=checked_root(&path,&known_roots())?;
        let _lock=install::lock_installation(&root)?;
        install::idle()?;
        Ok(DeletePreview{path:root.to_string_lossy().into_owned(),version:version(&root)?.0})
    }).await.map_err(|e|e.to_string())?
}

fn matches_path(value:&serde_json::Value,root:&Path)->bool {
    value.as_str().is_some_and(|s|s.replace('/',"\\").trim_end_matches('\\').eq_ignore_ascii_case(&root.to_string_lossy().replace('/',"\\").trim_end_matches('\\')))
}
fn unregister(root:&Path)->Result<(),String>{
    let backup=crate::storage::root().join("Backups/engine-removal").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&backup).map_err(io)?;
    let launcher=install::launcher_path();
    if launcher.exists(){
        no_links(&launcher)?;
        let bytes=fs::read(&launcher).map_err(io)?;
        let mut data:serde_json::Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        let list=data["InstallationList"].as_array_mut().ok_or("Повреждён список установок Epic")?;
        let len=list.len();list.retain(|e|!matches_path(&e["InstallLocation"],root));
        if list.len()!=len {fs::write(backup.join("LauncherInstalled.dat"),bytes).map_err(io)?;install::atomic(&launcher,&serde_json::to_vec(&data).map_err(|e|e.to_string())?)?;}
    }
    for (path,_) in items().into_iter().filter(|(_,i)|matches_path(&i["InstallLocation"],root)) {
        no_links(&path)?;
        fs::copy(&path,backup.join(path.file_name().ok_or("Имя записи Epic")?)).map_err(io)?;
        fs::remove_file(path).map_err(io)?;
    }
    { let base=crate::storage::settings();
        let path=base.join("engine-installs.json");
        if path.exists() {
            no_links(&path)?;
            let bytes=fs::read(&path).map_err(io)?;
            let mut roots:Vec<PathBuf>=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
            roots.retain(|p|!matches_path(&serde_json::Value::String(p.to_string_lossy().into_owned()),root));
            fs::write(backup.join("engine-installs.json"),bytes).map_err(io)?;
            install::atomic(&path,&serde_json::to_vec(&roots).map_err(|e|e.to_string())?)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn engine_delete(path:String,expected_version:String,state:tauri::State<'_,EngineState>)->Result<String,String>{
    if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции с движком.".into());}
    let _busy=install::Busy(&state.active);
    tauri::async_runtime::spawn_blocking(move||{
        let root=checked_root(&path,&known_roots())?;
        let _lock=install::lock_installation(&root)?;
        install::idle()?;
        if version(&root)?.0!=expected_version{return Err("Версия движка изменилась. Повторите подтверждение.".into());}
        install::idle()?;
        // The root has been resolved and checked against installed engines above.
        // remove_dir_all removes links themselves and does not traverse their targets.
        fs::remove_dir_all(&root).map_err(|e|format!("Не все файлы удалось удалить: {e}. Папка: {}",root.display()))?;
        if root.exists(){return Err("Папка движка осталась на диске.".into());}
        match unregister(&root) {
            Ok(())=>Ok("Движок удалён.".into()),
            Err(e)=>Ok(format!("Папка движка удалена, но не удалось убрать запись Epic: {e}")),
        }
    }).await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protects_unknown_roots_and_pending_updates(){
        let root=std::env::temp_dir().join(format!("um-engine-delete-test-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Engine/Binaries/Win64")).unwrap();fs::create_dir_all(root.join("Engine/Build")).unwrap();
        fs::write(root.join("Engine/Binaries/Win64/UnrealEditor.exe"),b"fixture").unwrap();
        fs::write(root.join("Engine/Build/Build.version"),br#"{"MajorVersion":5,"MinorVersion":8,"PatchVersion":3,"Changelist":1}"#).unwrap();
        let p=root.to_str().unwrap();
        assert!(checked_root(p,&[]).is_err());assert!(checked_root(p,&[root.clone()]).is_ok());
        fs::write(root.join(".um-engine-update.json"),b"{}").unwrap();assert!(checked_root(p,&[root.clone()]).is_err());fs::remove_file(root.join(".um-engine-update.json")).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn permanent_removal_includes_plugin_assets_and_nested_projects() {
        let base=std::env::temp_dir().join(format!("um-engine-delete-test-{}",uuid::Uuid::new_v4()));
        let root=base.join("engine");
        fs::create_dir_all(root.join("Engine/Plugins/Example/Content")).unwrap();
        fs::write(root.join("Engine/Plugins/Example/Content/Asset.uasset"), b"asset").unwrap();
        fs::write(root.join("Engine/Plugins/Example/Example.uproject"), b"{}").unwrap();
        fs::write(base.join("outside.uproject"), b"keep").unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(!root.exists());
        assert_eq!(fs::read(base.join("outside.uproject")).unwrap(), b"keep");
        fs::remove_dir_all(base).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn permanent_removal_does_not_follow_directory_junctions() {
        let base=std::env::temp_dir().join(format!("um-engine-delete-test-{}",uuid::Uuid::new_v4()));
        let root=base.join("engine");
        let outside=base.join("outside");
        fs::create_dir_all(&root).unwrap();fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let junction=root.join("PluginLink");
        let status=std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:UM_TEST_LINK -Target $env:UM_TEST_TARGET | Out-Null"])
            .env("UM_TEST_LINK", &junction).env("UM_TEST_TARGET", &outside).status().unwrap();
        assert!(status.success());
        fs::remove_dir_all(&root).unwrap();
        assert!(!root.exists());
        assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"keep");
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn registration_matches_exact_path_only(){
        let root=Path::new("E:/UNREAL/UE_5.8");
        assert!(matches_path(&serde_json::json!("e:\\unreal\\ue_5.8\\"),root));
        assert!(!matches_path(&serde_json::json!("E:/UNREAL/UE_5.8_other"),root));
    }
}

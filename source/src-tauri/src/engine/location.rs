//! Resolve a folder-picker selection without nesting an installed engine inside
//! another UE_<branch> directory. Adoption only updates UM's local index.
use super::*;

fn branch_folder(app_name: &str) -> Result<&str, String> {
    let branch = app_name
        .strip_prefix("UE_")
        .ok_or("Некорректная версия движка")?;
    let parts = branch.split('.').collect::<Vec<_>>();
    if parts.len() != 2
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Некорректная версия движка".into());
    }
    Ok(branch)
}

fn selected_location(path: &str, app_name: &str) -> Result<PathBuf, String> {
    branch_folder(app_name)?;
    let root = target(path)?;
    let build = root.join("Engine/Build/Build.version");
    no_links(&build)?;
    if build.is_file()
        || root
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(app_name))
    {
        return Ok(root);
    }
    Ok(root.join(app_name))
}

#[tauri::command]
pub fn engine_resolve_location(path: Option<String>, app_name: String) -> Result<String, String> {
    let branch = branch_folder(&app_name)?;
    if let Some(path) = path.filter(|p| !p.trim().is_empty()) {
        return selected_location(path.trim(), &app_name).map(|p| p.to_string_lossy().into_owned());
    }
    let installed = crate::scan_engines();
    if let Some(engine) = installed.iter().find(|e| e.version == branch) {
        return Ok(engine.path.clone());
    }
    let parent = installed
        .iter()
        .find_map(|e| Path::new(&e.path).parent().map(Path::to_path_buf))
        .or_else(|| dirs::data_local_dir().map(|p| p.join("UnrealEngine")))
        .ok_or("Не удалось выбрать папку установки. Укажите её вручную.")?;
    Ok(target(&parent.join(&app_name).to_string_lossy())?
        .to_string_lossy()
        .into_owned())
}

fn adoptable_root(path: &str) -> Result<PathBuf, String> {
    let root = target(path)?;
    version(&root)?;
    if root.join(".um-engine-update.json").exists() {
        return Err(
            "Предыдущая установка не завершена. Сначала продолжите или восстановите её.".into(),
        );
    }
    let bin = root.join("Engine/Binaries/Win64");
    let editor=[bin.join("UnrealEditor.exe"),bin.join("UE4Editor.exe")].into_iter().find(|p|p.is_file())
        .ok_or("В выбранной папке не найден редактор Unreal Engine. Для проверки неполной установки выберите эту папку в мастере установки нужной версии.")?;
    no_links(&editor)?;
    Ok(root)
}

#[tauri::command]
pub fn engine_adopt_location(
    path: String,
    state: tauri::State<'_, EngineState>,
) -> Result<crate::EngineInstall, String> {
    let _update_operation = crate::app_updates::operation()?;
    if state.active.swap(true, Ordering::SeqCst) {
        return Err("Дождитесь текущей операции с движком".into());
    }
    let _busy = install::Busy(&state.active);
    let root = adoptable_root(&path)?;
    let (version, _) = version(&root)?;
    register_root(&root)?;
    Ok(crate::EngineInstall {
        name: root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Unreal Engine")
            .into(),
        version: version
            .rsplit_once('.')
            .map(|(branch, _)| branch)
            .unwrap_or(&version)
            .into(),
        path: root.to_string_lossy().into_owned(),
        source: "Local installation".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("um-engine-location-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn cleanup(root: &Path) {
        assert!(root.is_absolute());
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("um-engine-location-"));
        no_links(root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn folder_picker_preserves_installed_and_named_root_and_appends_to_parent() {
        let root = fixture();
        let installed = root.join("Existing custom name");
        fs::create_dir_all(installed.join("Engine/Build")).unwrap();
        fs::write(installed.join("Engine/Build/Build.version"), b"{}").unwrap();
        assert_eq!(
            selected_location(&installed.to_string_lossy(), "UE_5.8").unwrap(),
            installed
        );
        assert_eq!(
            selected_location(&root.join("UE_5.8").to_string_lossy(), "UE_5.8").unwrap(),
            root.join("UE_5.8")
        );
        assert_eq!(
            selected_location(&root.to_string_lossy(), "UE_5.8").unwrap(),
            root.join("UE_5.8")
        );
        for invalid in ["UE_../escape", "UE_5.8/escape", "UE_5", "5.8"] {
            assert!(selected_location(&root.to_string_lossy(), invalid).is_err());
        }
        cleanup(&root);
    }
    #[test]
    fn adoption_validates_version_editor_and_pending_operation_without_mutation() {
        let root = fixture();
        fs::create_dir_all(root.join("Engine/Build")).unwrap();
        fs::write(
            root.join("Engine/Build/Build.version"),
            br#"{"MajorVersion":5,"MinorVersion":8,"PatchVersion":3,"Changelist":123}"#,
        )
        .unwrap();
        assert!(adoptable_root(&root.to_string_lossy()).is_err());
        fs::create_dir_all(root.join("Engine/Binaries/Win64")).unwrap();
        fs::write(
            root.join("Engine/Binaries/Win64/UnrealEditor.exe"),
            b"fixture",
        )
        .unwrap();
        assert_eq!(adoptable_root(&root.to_string_lossy()).unwrap(), root);
        fs::write(root.join(".um-engine-update.json"), b"pending").unwrap();
        assert!(adoptable_root(&root.to_string_lossy()).is_err());
        assert_eq!(
            fs::read(root.join("Engine/Binaries/Win64/UnrealEditor.exe")).unwrap(),
            b"fixture"
        );
        cleanup(&root);
    }
}

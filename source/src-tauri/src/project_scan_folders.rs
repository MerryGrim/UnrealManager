//! Explicit discovery preferences; these paths confer no deletion authority.
use std::{collections::BTreeSet, fs::File, io::Read, path::{Component, Path, PathBuf}, sync::Mutex};
use serde::{Deserialize, Serialize};

const MAX_FOLDERS: usize = 16;
const MAX_BYTES: u64 = 64 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Folders { schema: u32, folders: Vec<String> }

#[derive(Serialize)]
pub(crate) struct RefreshResult {
    projects: Vec<crate::UnrealProject>,
    summary: crate::project_discovery::DiscoveryResult,
}

fn location() -> PathBuf { crate::storage::settings().join("project-scan-folders.json") }
fn error(cause: impl std::fmt::Display) -> String { format!("Не удалось прочитать или сохранить папки проектов: {cause}") }
fn shape(path: &Path) -> bool {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir))
        || path.as_os_str().len() > 32767 || path.to_string_lossy().chars().any(char::is_control) { return false; }
    #[cfg(windows)] {
        use std::path::Prefix;
        if !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))) { return false; }
    }
    true
}
fn key(path: &Path) -> String { path.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase() }

fn validate_stored(stored: Folders) -> Result<Vec<PathBuf>, String> {
    if stored.schema != 1 || stored.folders.len() > MAX_FOLDERS { return Err(error("Некорректный список папок.")); }
    let mut seen = BTreeSet::new(); let mut result = Vec::new();
    for raw in stored.folders {
        let folder = PathBuf::from(raw);
        if !shape(&folder) || !seen.insert(key(&folder)) { return Err(error("Некорректный список папок.")); }
        result.push(folder);
    }
    Ok(result)
}

fn folders_at(path: &Path, defaults: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let _guard = LOCK.lock().map_err(error)?;
    crate::vault::no_links(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
            let mut seen = BTreeSet::new();
            return Ok(defaults.iter().filter_map(|p| crate::project_discovery::checked_root(p).ok())
                .filter(|p| seen.insert(key(p))).take(MAX_FOLDERS).collect());
        }
        Err(cause) => return Err(error(cause)),
    };
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES { return Err(error("Некорректный размер списка папок.")); }
    let mut bytes = Vec::new(); file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(error("Некорректный размер списка папок.")); }
    // Preserve disconnected/missing configured directories in preferences.
    // The scanner rechecks local drive type and links before any traversal.
    validate_stored(serde_json::from_slice(&bytes).map_err(error)?)
}

fn set_at(path: &Path, folders: Vec<String>) -> Result<Vec<PathBuf>, String> {
    let _guard = LOCK.lock().map_err(error)?;
    if folders.len() > MAX_FOLDERS { return Err("Можно выбрать не более 16 папок проектов.".into()); }
    let mut seen = BTreeSet::new(); let mut checked = Vec::new();
    for folder in folders {
        let folder = Path::new(&folder);
        if !shape(folder) { return Err("Укажите существующую локальную папку полным путём.".into()); }
        let folder = crate::project_discovery::checked_root(folder)?;
        if seen.insert(key(&folder)) { checked.push(folder); }
    }
    let stored = Folders { schema: 1, folders: checked.iter().map(|p| p.to_string_lossy().into_owned()).collect() };
    let bytes = serde_json::to_vec_pretty(&stored).map_err(error)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(error("Список папок слишком большой.")); }
    crate::vault::no_links(path)?;
    // A malformed existing preference is reported instead of silently discarded.
    match File::open(path) {
        Ok(file) => {
            if file.metadata().map_err(error)?.len() > MAX_BYTES { return Err(error("Некорректный размер списка папок.")); }
            let mut old = Vec::new(); file.take(MAX_BYTES + 1).read_to_end(&mut old).map_err(error)?;
            if old.len() as u64 > MAX_BYTES { return Err(error("Некорректный размер списка папок.")); }
            let current = validate_stored(serde_json::from_slice(&old).map_err(error)?)?;
            if current == checked { return Ok(checked); }
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => (),
        Err(cause) => return Err(error(cause)),
    }
    crate::engine::install::atomic(path, &bytes).map_err(error)?;
    Ok(checked)
}

fn strings(paths: Vec<PathBuf>) -> Vec<String> { paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect() }

#[tauri::command]
pub(crate) async fn list_scan_folders() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| folders_at(&location(), &crate::project_roots()).map(strings))
        .await.map_err(|_| "Не удалось прочитать папки проектов.".to_string())?
}

#[tauri::command]
pub(crate) async fn set_scan_folders(state: tauri::State<'_, crate::project_discovery::ProjectDiscoveryState>, folders: Vec<String>) -> Result<Vec<String>, String> {
    let operation = crate::app_updates::operation()?;
    let lease = state.reserve(uuid::Uuid::new_v4().to_string())?;
    tauri::async_runtime::spawn_blocking(move || { let _operation = operation; let _lease = lease; set_at(&location(), folders).map(strings) })
        .await.map_err(|_| "Не удалось сохранить папки проектов.".to_string())?
}

#[tauri::command]
pub(crate) async fn refresh_projects(state: tauri::State<'_, crate::project_discovery::ProjectDiscoveryState>, id: String, on_progress: tauri::ipc::Channel<crate::project_discovery::DiscoveryProgress>) -> Result<RefreshResult, String> {
    let operation = crate::app_updates::operation()?;
    let lease = state.reserve(id.clone())?;
    tauri::async_runtime::spawn_blocking(move || -> Result<RefreshResult, String> {
        let _operation = operation; let _lease = lease;
        // Fail before traversal if the exact-project registry cannot be loaded.
        let _known = crate::project_registry::paths()?;
        let folders = folders_at(&location(), &crate::project_roots())?;
        let summary = crate::project_discovery::scan_configured(&folders, id, &_lease, |p| { let _ = on_progress.send(p); }, crate::project_registry::register_discovered)?;
        let known = crate::project_registry::paths()?;
        Ok(RefreshResult { projects: crate::scan_projects_in(&[], &known), summary })
    }).await.map_err(|_| "Поиск проектов прерван.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::project_registry::tests::Fixture;
    fn record(f: &Fixture) -> PathBuf { f.root.join("Settings/project-scan-folders.json") }
    fn folder(f: &Fixture, relative: &str) -> PathBuf { let p = f.root.join(relative); fs::create_dir_all(&p).unwrap(); p }
    fn descriptor(root: &Path, name: &str) -> PathBuf { let p = root.join(name); fs::write(&p, br#"{"FileVersion":3,"EngineAssociation":"5.8"}"#).unwrap(); p }

    #[test]
    fn absent_preferences_offer_existing_defaults_without_writing_and_explicit_empty_stays_empty() {
        let f = Fixture::new(); let selected = folder(&f, "Проекты 空白");
        let defaults = [selected.clone(), f.root.join("missing"), selected.clone()];
        assert_eq!(folders_at(&record(&f), &defaults).unwrap().len(), 1); assert!(!record(&f).exists());
        assert!(set_at(&record(&f), Vec::new()).unwrap().is_empty());
        assert!(folders_at(&record(&f), &defaults).unwrap().is_empty());
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(record(&f)).unwrap()).unwrap()["folders"], serde_json::json!([]));
    }

    #[test]
    fn preferences_roundtrip_canonical_unicode_folders_and_deduplicate_aliases() {
        let f = Fixture::new(); let selected = folder(&f, "Проекты 空白");
        let mut aliases = vec![selected.to_string_lossy().into_owned(), selected.join(".").to_string_lossy().into_owned()];
        #[cfg(windows)] aliases.push(selected.to_string_lossy().to_uppercase());
        let saved = set_at(&record(&f), aliases).unwrap(); assert_eq!(saved.len(), 1);
        assert_eq!(saved, folders_at(&record(&f), &[]).unwrap());
        assert!(!f.record().exists(), "Folder choices must not register directories as projects");
    }

    #[test]
    fn invalid_or_excessive_choices_never_replace_existing_preferences() {
        let f = Fixture::new(); let kept = folder(&f, "kept"); set_at(&record(&f), strings(vec![kept.clone()])).unwrap();
        let old = fs::read(record(&f)).unwrap(); let file = descriptor(&kept, "File.uproject");
        for candidate in ["relative".into(), f.root.join("missing").to_string_lossy().into_owned(), file.to_string_lossy().into_owned(), "bad\0path".into()] {
            assert!(set_at(&record(&f), vec![candidate]).is_err()); assert_eq!(fs::read(record(&f)).unwrap(), old);
        }
        assert!(set_at(&record(&f), vec![kept.to_string_lossy().into_owned(); MAX_FOLDERS + 1]).is_err());
        assert_eq!(fs::read(record(&f)).unwrap(), old);
        #[cfg(windows)] for candidate in [r"\\server\share\Projects", r"\\?\UNC\server\share\Projects", r"\\.\C:\Projects"] { assert!(!shape(Path::new(candidate))); }
    }

    #[test]
    fn sixteen_distinct_choices_are_supported_and_the_next_does_not_change_saved_bytes() {
        let f = Fixture::new(); let folders: Vec<_> = (0..MAX_FOLDERS).map(|n| folder(&f, &format!("choice-{n}"))).collect();
        assert_eq!(set_at(&record(&f), strings(folders.clone())).unwrap().len(), MAX_FOLDERS);
        let old = fs::read(record(&f)).unwrap(); let mut excessive = strings(folders); excessive.push(folder(&f, "choice-extra").to_string_lossy().into_owned());
        assert!(set_at(&record(&f), excessive).is_err()); assert_eq!(fs::read(record(&f)).unwrap(), old);
    }

    #[test]
    fn corrupt_and_oversized_preferences_are_not_silently_overwritten() {
        let f = Fixture::new(); let selected = folder(&f, "chosen"); fs::create_dir_all(record(&f).parent().unwrap()).unwrap();
        for bytes in [br#"{"schema":2,"folders":[]}"#.to_vec(), br#"{"schema":1,"folders":[],"unexpected":true}"#.to_vec(), vec![b' '; MAX_BYTES as usize + 1]] {
            fs::write(record(&f), &bytes).unwrap(); assert!(folders_at(&record(&f), &[]).is_err());
            assert!(set_at(&record(&f), strings(vec![selected.clone()])).is_err()); assert_eq!(fs::read(record(&f)).unwrap(), bytes);
        }
    }

    #[cfg(windows)]
    #[test]
    fn locked_preferences_fail_atomically_and_can_be_saved_after_unlock() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new(); let first = folder(&f, "first"); let second = folder(&f, "second");
        set_at(&record(&f), strings(vec![first])).unwrap(); let before = fs::read(record(&f)).unwrap();
        let guard = fs::OpenOptions::new().read(true).share_mode(1).open(record(&f)).unwrap();
        assert!(set_at(&record(&f), strings(vec![second.clone()])).is_err());
        assert_eq!(fs::read(record(&f)).unwrap(), before);
        assert_eq!(fs::read_dir(record(&f).parent().unwrap()).unwrap().count(), 1, "Failed atomic write must remove its own temporary");
        drop(guard); set_at(&record(&f), strings(vec![second])).unwrap();
        assert_ne!(fs::read(record(&f)).unwrap(), before);
    }

    #[test]
    fn remembered_empty_folder_finds_later_sibling_projects_without_reselecting_and_keeps_known_projects() {
        let f = Fixture::new(); let selected = folder(&f, "chosen"); let old_dir = folder(&f, "outside");
        let old = descriptor(&old_dir, "Original.uproject"); f.register(&[old.clone()]).unwrap();
        set_at(&record(&f), strings(vec![selected.clone()])).unwrap();
        let state = crate::project_discovery::ProjectDiscoveryState::default();
        for n in 0..3 {
            if n > 0 { descriptor(&selected, &format!("New{n}.uproject")); }
            let id = uuid::Uuid::new_v4().to_string(); let lease = state.reserve(id.clone()).unwrap();
            crate::project_discovery::scan_configured(&folders_at(&record(&f), &[]).unwrap(), id, &lease, |_| {}, |paths| f.register_count(paths)).unwrap();
            assert_eq!(f.paths().len(), n + 1); assert!(f.paths().contains(&crate::project_registry::display_path(old.canonicalize().unwrap())));
            drop(lease);
        }
        let fresh = crate::scan_projects_in(&[], &f.paths()); assert_eq!(fresh.len(), 3);
        assert_eq!(folders_at(&record(&f), &[]).unwrap().len(), 1);
    }

    #[test]
    fn unavailable_configured_folder_is_preserved_and_reported_without_removing_known_projects() {
        let f = Fixture::new(); let selected = folder(&f, "chosen"); let known_dir = folder(&f, "outside");
        let known = descriptor(&known_dir, "Kept.uproject"); f.register(&[known]).unwrap();
        set_at(&record(&f), strings(vec![selected.clone()])).unwrap(); let before = fs::read(record(&f)).unwrap(); fs::remove_dir(&selected).unwrap();
        let roots = folders_at(&record(&f), &[]).unwrap(); assert_eq!(roots.len(), 1);
        let state = crate::project_discovery::ProjectDiscoveryState::default(); let id = uuid::Uuid::new_v4().to_string(); let lease = state.reserve(id.clone()).unwrap();
        let mut terminal = None;
        crate::project_discovery::scan_configured(&roots, id, &lease, |progress| terminal = Some(serde_json::to_value(progress).unwrap()), |_| panic!("No newly found project")).unwrap();
        let terminal = terminal.unwrap(); assert_eq!(terminal["phase"], "done"); assert_eq!(terminal["skipped"], 1); assert_eq!(terminal["errors"].as_array().unwrap().len(), 1);
        assert_eq!(fs::read(record(&f)).unwrap(), before); assert_eq!(f.paths().len(), 1);
    }
}

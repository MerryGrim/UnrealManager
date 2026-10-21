use std::{collections::BTreeSet, fs::File, io::Read, path::{Component, Path, PathBuf}, sync::Mutex};
use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PROJECTS: usize = 4096;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry { schema: u32, paths: Vec<String> }

fn location() -> PathBuf { crate::storage::settings().join("projects.json") }
fn error(error: impl std::fmt::Display) -> String { format!("Project list: {error}") }

fn path_shape(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir))
        && path.as_os_str().len() <= 32767 && !path.to_string_lossy().contains('\0')
        && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("uproject"))
        && path.parent().is_some()
}

pub(crate) fn display_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)] {
        let raw = path.to_string_lossy();
        if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{unc}")); }
        if let Some(disk) = raw.strip_prefix(r"\\?\") { return PathBuf::from(disk); }
    }
    path
}

pub(crate) fn path_key(path: &Path) -> String {
    display_path(path.canonicalize().unwrap_or_else(|_| path.to_path_buf())).to_string_lossy().replace('\\', "/").to_lowercase()
}

pub(crate) fn checked_project(path: &Path) -> Result<PathBuf, String> {
    if !path_shape(path) { return Err("Выберите файл .uproject".into()); }
    crate::vault::no_links(path)?;
    let file = File::open(path).map_err(error)?;
    if !file.metadata().map_err(error)?.is_file() { return Err("Файл проекта не найден".into()); }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > MAX_BYTES { return Err("Не удалось прочитать проект".into()); }
    let descriptor: serde_json::Value = serde_json::from_slice(&bytes).map_err(error)?;
    if !descriptor.is_object() { return Err("Некорректный файл проекта".into()); }
    let canonical = display_path(path.canonicalize().map_err(error)?);
    crate::vault::no_links(&canonical)?;
    Ok(canonical)
}

fn read(path: &Path) -> Result<Registry, String> {
    crate::vault::no_links(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Registry { schema: 1, paths: Vec::new() }),
        Err(cause) => return Err(error(cause)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(error("project list is too large")); }
    let registry: Registry = serde_json::from_slice(&bytes).map_err(error)?;
    if registry.schema != 1 || registry.paths.len() > MAX_PROJECTS || registry.paths.iter().any(|p| !path_shape(Path::new(p))) {
        return Err(error("invalid project list"));
    }
    Ok(registry)
}

fn register_count_at(path: &Path, projects: &[PathBuf]) -> Result<usize, String> {
    let _guard = LOCK.lock().map_err(error)?;
    let checked = projects.iter().map(|p| checked_project(p)).collect::<Result<Vec<_>, _>>()?;
    let mut registry = read(path)?;
    let mut seen: BTreeSet<_> = registry.paths.iter().map(|p| path_key(Path::new(p))).collect();
    let mut added = 0;
    for project in checked {
        if seen.insert(path_key(&project)) { registry.paths.push(project.to_string_lossy().into_owned()); added += 1; }
    }
    if added == 0 { return Ok(0); }
    if registry.paths.len() > MAX_PROJECTS { return Err("Список проектов заполнен (4096). Новые пути не сохранены.".into()); }
    let bytes = serde_json::to_vec_pretty(&registry).map_err(error)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(error("project list is too large")); }
    crate::engine::install::atomic(path, &bytes)?;
    Ok(added)
}

pub(crate) fn register_created(projects: &[PathBuf]) -> Result<(), String> { register_count_at(&location(), projects).map(|_| ()) }
pub(crate) fn register_discovered(projects: &[PathBuf]) -> Result<usize, String> { register_count_at(&location(), projects) }

fn paths_at(path: &Path) -> Result<Vec<PathBuf>, String> {
    let _guard = LOCK.lock().map_err(error)?;
    // A disconnected drive or deleted descriptor is omitted, never pruned from the saved list.
    Ok(read(path)?.paths.into_iter().filter_map(|p| checked_project(Path::new(&p)).ok()).collect())
}

pub(crate) fn paths() -> Result<Vec<PathBuf>, String> { paths_at(&location()) }

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{fs, io::Write};

    pub(crate) struct Fixture { pub(crate) root: PathBuf, owner: String, ready: bool }
    impl Fixture {
        pub(crate) fn new() -> Self {
            let owner = uuid::Uuid::new_v4().to_string();
            let root = std::env::temp_dir().join(format!("um-project-registry-{}-{owner}", std::process::id()));
            fs::create_dir(&root).unwrap();
            let mut fixture = Self { root, owner, ready: false };
            let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(fixture.root.join(".owner")).unwrap();
            marker.write_all(fixture.owner.as_bytes()).unwrap(); drop(marker); fixture.ready = true;
            fixture
        }
        pub(crate) fn record(&self) -> PathBuf { self.root.join("Settings/projects.json") }
        pub(crate) fn register(&self, projects: &[PathBuf]) -> Result<(), String> { self.register_count(projects).map(|_| ()) }
        pub(crate) fn register_count(&self, projects: &[PathBuf]) -> Result<usize, String> { register_count_at(&self.record(), projects) }
        pub(crate) fn paths(&self) -> Vec<PathBuf> { paths_at(&self.record()).unwrap() }
        fn project(&self, relative: &str) -> PathBuf {
            let path = self.root.join(relative); fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, br#"{"FileVersion":3,"EngineAssociation":"5.8"}"#).unwrap(); path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if crate::vault::no_links(&self.root).is_err() { return; }
            let marker = self.root.join(".owner");
            if !self.ready {
                let entries = match fs::read_dir(&self.root).and_then(|entries| entries.collect::<Result<Vec<_>, _>>()) { Ok(entries) => entries, Err(_) => return };
                if entries.is_empty() { let _ = fs::remove_dir(&self.root); }
                else if entries.len() == 1 && entries[0].path() == marker && crate::vault::no_links(&marker).is_ok() {
                    if fs::read(&marker).is_ok_and(|bytes| self.owner.as_bytes().starts_with(&bytes)) { let _ = fs::remove_file(&marker); let _ = fs::remove_dir(&self.root); }
                }
                return;
            }
            if fs::read_to_string(&marker).ok().as_deref() != Some(&self.owner) { return; }
            let mut files = Vec::new(); let mut directories = Vec::new(); let mut bytes = 0u64;
            for entry in walkdir::WalkDir::new(&self.root).min_depth(1).follow_links(false) {
                let Ok(entry) = entry else { return; };
                if crate::vault::no_links(entry.path()).is_err() { return; }
                let Ok(meta) = entry.metadata() else { return; };
                if meta.is_dir() { directories.push(entry.path().to_path_buf()); }
                else if meta.is_file() { bytes = bytes.saturating_add(meta.len()); files.push(entry.path().to_path_buf()); }
                else { return; }
                if files.len() + directories.len() > 128 || bytes > 4 * MAX_BYTES { return; }
            }
            for path in files { if fs::remove_file(path).is_err() { return; } }
            directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
            for path in directories { if fs::remove_dir(path).is_err() { return; } }
            let _ = fs::remove_dir(&self.root);
        }
    }

    #[test]
    fn outside_default_roots_persists_exact_unicode_path_and_deduplicates() {
        let fixture = Fixture::new();
        let project = fixture.project("arbitrary local folder/Проекты 空白/My Game.uproject");
        fixture.register(&[project.clone()]).unwrap();
        let first = fs::read(fixture.record()).unwrap();
        let alias = project.parent().unwrap().join(".").join("My Game.uproject");
        fixture.register(&[alias]).unwrap();
        #[cfg(windows)] fixture.register(&[PathBuf::from(project.to_string_lossy().to_uppercase())]).unwrap();
        assert_eq!(fs::read(fixture.record()).unwrap(), first);
        // No process-local registry cache: each load reconstructs the saved exact paths.
        assert_eq!(paths_at(&fixture.record()).unwrap(), vec![display_path(project.canonicalize().unwrap())]);
        assert_eq!(paths_at(&fixture.record()).unwrap(), fixture.paths());
        let listed = crate::scan_projects_in(&[], &fixture.paths());
        assert_eq!(listed.len(), 1); assert_eq!(listed[0].name, "My Game");
        let merged = crate::scan_projects_in(&[fixture.root.clone()], &fixture.paths());
        assert_eq!(merged.len(), 1); assert_eq!(merged[0].engine_association.as_deref(), Some("5.8"));
    }

    #[test]
    fn missing_or_invalid_descriptors_are_skipped_without_mutating_registry_or_projects() {
        let fixture = Fixture::new();
        let kept = fixture.project("one/Kept.uproject");
        let removed = fixture.project("two/Removed.uproject");
        fixture.register(&[kept.clone(), removed.clone()]).unwrap();
        let before = fs::read(fixture.record()).unwrap();
        fs::remove_file(&removed).unwrap();
        fs::write(&kept, b"not a descriptor").unwrap();
        assert!(fixture.paths().is_empty());
        assert_eq!(fs::read(fixture.record()).unwrap(), before);
        assert_eq!(fs::read(&kept).unwrap(), b"not a descriptor");
        fs::write(&kept, b"{}").unwrap();
        assert_eq!(fixture.paths(), vec![display_path(kept.canonicalize().unwrap())]);
    }

    #[test]
    fn invalid_registration_or_corrupt_saved_list_is_never_overwritten() {
        let fixture = Fixture::new();
        let project = fixture.project("existing/Good.uproject");
        for invalid in [PathBuf::from("relative.uproject"), fixture.root.join("Missing.uproject"), fixture.root.join("Wrong.txt")] {
            assert!(fixture.register(&[invalid]).is_err()); assert!(!fixture.record().exists());
        }
        let bad = fixture.project("invalid/Bad.uproject"); fs::write(&bad, b"[]").unwrap();
        assert!(fixture.register(&[project.clone(), bad]).is_err()); assert!(!fixture.record().exists());
        fs::create_dir_all(fixture.record().parent().unwrap()).unwrap(); fs::write(fixture.record(), b"broken registry").unwrap();
        assert!(fixture.register(&[project]).is_err());
        assert_eq!(fs::read(fixture.record()).unwrap(), b"broken registry");
    }
    #[test]
    fn exact_project_file_at_volume_root_is_not_a_directory_registration() {
        #[cfg(windows)] { assert!(path_shape(Path::new("Q:/Demo.uproject"))); assert!(!path_shape(Path::new("Q:/"))); }
        #[cfg(not(windows))] { assert!(path_shape(Path::new("/Demo.uproject"))); assert!(!path_shape(Path::new("/"))); }
    }
}

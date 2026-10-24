//! Explicit, finite local project discovery. Only the app's exact-path list is written.
use std::{collections::{BTreeSet, VecDeque}, fs, path::{Component, Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicU8, Ordering}}, time::{Duration, Instant}};
use serde::Serialize;

const SCANNING: u8 = 0;
const CANCELLED: u8 = 1;
const SAVING: u8 = 2;
const FINISHED: u8 = 3;
const BUSY: &str = "Поиск проектов уже выполняется.";
const ROOT_ERROR: &str = "Укажите существующую папку или диск полным путём.";
const NETWORK_ERROR: &str = "Поиск по сетевым путям пока не поддерживается.";

struct Control { phase: AtomicU8 }
impl Control {
    fn new(cancelled: bool) -> Self { Self { phase: AtomicU8::new(if cancelled { CANCELLED } else { SCANNING }) } }
    fn cancelled(&self) -> bool { self.phase.load(Ordering::SeqCst) == CANCELLED }
    fn cancel(&self) -> bool {
        match self.phase.compare_exchange(SCANNING, CANCELLED, Ordering::SeqCst, Ordering::SeqCst) { Ok(_) | Err(CANCELLED) => true, _ => false }
    }
    fn start_save(&self) -> bool {
        loop {
            let previous = self.phase.load(Ordering::SeqCst);
            if self.phase.compare_exchange(previous, SAVING, Ordering::SeqCst, Ordering::SeqCst).is_ok() { return previous == CANCELLED; }
        }
    }
}
#[derive(Default)]
struct Jobs { active: Option<(String, Arc<Control>)>, cancelled_before_start: VecDeque<String> }
#[derive(Default)]
pub(crate) struct ProjectDiscoveryState { jobs: Arc<Mutex<Jobs>> }
pub(crate) struct Lease { jobs: Arc<Mutex<Jobs>>, id: String, control: Arc<Control> }
impl ProjectDiscoveryState {
    pub(crate) fn reserve(&self, id: String) -> Result<Lease, String> {
        if !uuid::Uuid::parse_str(&id).is_ok_and(|v| v.to_string() == id) { return Err("Некорректный идентификатор поиска проектов.".into()); }
        let mut jobs = self.jobs.lock().map_err(|_| BUSY)?;
        if jobs.active.is_some() { return Err(BUSY.into()); }
        let cancelled = jobs.cancelled_before_start.iter().position(|v| v == &id).map(|index| jobs.cancelled_before_start.remove(index)).is_some();
        let control = Arc::new(Control::new(cancelled)); jobs.active = Some((id.clone(), control.clone()));
        Ok(Lease { jobs: self.jobs.clone(), id, control })
    }
    fn cancel(&self, id: &str) -> bool {
        if !uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) { return false; }
        let Ok(mut jobs) = self.jobs.lock() else { return false; };
        if let Some((active, control)) = &jobs.active { return active == id && control.cancel(); }
        // A bounded volatile list covers cancel IPC arriving immediately before its scan.
        if !jobs.cancelled_before_start.iter().any(|v| v == id) {
            if jobs.cancelled_before_start.len() == 16 { jobs.cancelled_before_start.pop_front(); }
            jobs.cancelled_before_start.push_back(id.into());
        }
        true
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.control.phase.store(FINISHED, Ordering::SeqCst);
        if let Ok(mut jobs) = self.jobs.lock() { if jobs.active.as_ref().is_some_and(|(id, _)| id == &self.id) { jobs.active = None; } }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryProgress {
    id: String, directories: usize, scanned_entries: usize, found: usize, skipped: usize,
    current_directory: String, phase: &'static str,
    limited: bool, limit_reason: Option<&'static str>, errors: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryResult {
    id: String, root: String, directories: usize, scanned_entries: usize, found: usize, added: usize,
    skipped: usize, cancelled: bool, limited: bool, limit_reason: Option<&'static str>, errors: Vec<String>,
}
#[derive(Clone, Copy)]
struct Options { depth: usize, entries: usize, projects: usize, time: Duration, interval: Duration }
impl Default for Options {
    fn default() -> Self { Self { depth: 16, entries: 100_000, projects: 4096, time: Duration::from_secs(120), interval: Duration::from_millis(250) } }
}
fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { meta.file_type().is_symlink() }
}
fn no_links(path: &Path) -> Result<(), String> {
    for part in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(part).map_err(|e| format!("{}: {e}", part.display()))?;
        if reparse(&metadata) { return Err(format!("Поиск через ссылку не поддерживается: {}", part.display())); }
    }
    Ok(())
}
pub(crate) fn checked_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) || path.to_string_lossy().contains('\0') { return Err(ROOT_ERROR.into()); }
    #[cfg(windows)] {
        use std::path::Prefix;
        use std::os::windows::ffi::OsStrExt;
        let Some(Component::Prefix(prefix)) = path.components().next() else { return Err(ROOT_ERROR.into()); };
        let drive = match prefix.kind() { Prefix::Disk(d) | Prefix::VerbatimDisk(d) => d, _ => return Err(NETWORK_ERROR.into()) };
        let drive_root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\", drive as char)).encode_wide().chain([0]).collect();
        let kind = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(drive_root.as_ptr()) };
        if kind == 4 { return Err(NETWORK_ERROR.into()); }
        if !matches!(kind, 2 | 3 | 5 | 6) { return Err(ROOT_ERROR.into()); }
    }
    no_links(path)?;
    if !fs::metadata(path).map_err(|_| ROOT_ERROR)?.is_dir() { return Err(ROOT_ERROR.into()); }
    Ok(crate::project_registry::display_path(path.canonicalize().map_err(|_| ROOT_ERROR)?))
}
fn always_skip(name: &str) -> bool { name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case("node_modules") }
fn generated(name: &str) -> bool { ["Content", "Engine", "Intermediate", "Saved", "Binaries", "DerivedDataCache"].iter().any(|v| name.eq_ignore_ascii_case(v)) }
fn engine_marker(root: &Path, children: &[PathBuf]) -> bool {
    if !children.iter().any(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Engine"))) { return false; }
    let build = root.join("Engine/Build/Build.version");
    let editor = root.join("Engine/Binaries/Win64/UnrealEditor.exe");
    let editor4 = root.join("Engine/Binaries/Win64/UE4Editor.exe");
    no_links(&build).is_ok() && build.is_file() && [editor, editor4].iter().any(|p| no_links(p).is_ok() && p.is_file())
}
fn scan(root: &Path, id: String, control: &Control, options: Options, mut notify: impl FnMut(DiscoveryProgress), register: impl FnOnce(&[PathBuf]) -> Result<usize, String>) -> Result<DiscoveryResult, String> {
    let root = checked_root(root)?;
    scan_checked(&[root], Vec::new(), id, control, options, &mut notify, register)
}

// Folder preferences are discovery scope only. They never extend the separate
// project deletion allowlist. One worker, budget, cancellation and atomic save
// cover the whole configured set, including a cancellation between roots.
pub(crate) fn scan_configured(roots: &[PathBuf], id: String, lease: &Lease, notify: impl FnMut(DiscoveryProgress), register: impl FnOnce(&[PathBuf]) -> Result<usize, String>) -> Result<DiscoveryResult, String> {
    let mut checked = Vec::new(); let mut errors = Vec::new();
    if !lease.control.cancelled() {
        for root in roots {
            if lease.control.cancelled() { break; }
            match checked_root(root) {
                Ok(root) => checked.push(root),
                Err(error) => { if errors.len() < 20 { errors.push(format!("{}: {error}", root.display())); } }
            }
        }
    }
    scan_checked(&checked, errors, id, &lease.control, Options::default(), notify, register)
}

fn scan_checked(roots: &[PathBuf], errors: Vec<String>, id: String, control: &Control, options: Options, mut notify: impl FnMut(DiscoveryProgress), register: impl FnOnce(&[PathBuf]) -> Result<usize, String>) -> Result<DiscoveryResult, String> {
    let started = Instant::now();
    let mut result = DiscoveryResult { id, root: roots.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), directories: 0, scanned_entries: 0, found: 0, added: 0, skipped: errors.len(), cancelled: false, limited: false, limit_reason: None, errors };
    let mut found = Vec::new(); let mut seen = BTreeSet::new(); let mut pending: Vec<_> = roots.iter().rev().map(|root| (root.clone(), 0usize)).collect(); let mut last = Instant::now();
    let emit = |result: &DiscoveryResult, current: &Path, phase| DiscoveryProgress { id: result.id.clone(), directories: result.directories, scanned_entries: result.scanned_entries, found: result.found, skipped: result.skipped, current_directory: current.to_string_lossy().into_owned(), phase, limited: result.limited, limit_reason: result.limit_reason, errors: result.errors.clone() };
    let mut current = PathBuf::from(&result.root); notify(emit(&result, &current, "scan"));
    'walk: while let Some((directory, depth)) = pending.pop() {
        if control.cancelled() { break; }
        if started.elapsed() >= options.time { result.limited = true; result.limit_reason = Some("time"); break; }
        current = directory.clone();
        if last.elapsed() >= options.interval { notify(emit(&result, &current, "scan")); last = Instant::now(); }
        let entries = match no_links(&directory).and_then(|_| fs::read_dir(&directory).map_err(|e| format!("{}: {e}", directory.display()))) {
            Ok(entries) => entries, Err(error) => {
                result.skipped += 1; if result.errors.len() < 20 { result.errors.push(error); }
                if last.elapsed() >= options.interval { notify(emit(&result, &current, "scan")); last = Instant::now(); }
                continue;
            }
        };
        result.directories += 1; let mut children = Vec::new(); let mut project_parent = false;
        for entry in entries {
            if control.cancelled() { break 'walk; }
            if started.elapsed() >= options.time { result.limited = true; result.limit_reason = Some("time"); break 'walk; }
            if result.scanned_entries >= options.entries { result.limited = true; result.limit_reason = Some("entries"); break 'walk; }
            result.scanned_entries += 1;
            let processed = (|| -> Result<(), String> {
                let entry = entry.map_err(|e| format!("{}: {e}", directory.display()))?; let path = entry.path();
                let metadata = fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                if reparse(&metadata) { result.skipped += 1; return Ok(()); }
                if metadata.is_dir() { children.push(path); }
                else if metadata.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("uproject")) {
                    no_links(&path)?;
                    let project = crate::project_registry::checked_project(&path).map_err(|cause| format!("Не удалось проверить проект {}: {cause}", path.display()))?;
                    project_parent = true;
                    if seen.insert(crate::project_registry::path_key(&project)) { found.push(project); result.found = found.len(); }
                }
                Ok(())
            })();
            if let Err(error) = processed { result.skipped += 1; if result.errors.len() < 20 { result.errors.push(error); } }
            if last.elapsed() >= options.interval { notify(emit(&result, &current, "scan")); last = Instant::now(); }
            if control.cancelled() { break 'walk; }
            if result.found >= options.projects { result.limited = true; result.limit_reason = Some("projects"); break 'walk; }
        }
        let recognized = project_parent || engine_marker(&directory, &children);
        children.sort();
        for child in children.into_iter().rev() {
            let name = child.file_name().unwrap_or_default().to_string_lossy();
            if always_skip(&name) || (recognized && generated(&name)) { result.skipped += 1; continue; }
            if depth >= options.depth { result.limited = true; result.limit_reason.get_or_insert("depth"); result.skipped += 1; continue; }
            pending.push((child, depth + 1));
        }
    }
    // Cancel stops traversal, but already found valid paths are saved once. The
    // phase transition makes a late cancel during the atomic save return false.
    result.cancelled = control.start_save(); notify(emit(&result, &current, "save"));
    if !found.is_empty() { result.added = register(&found).map_err(|e| format!("Не удалось сохранить список найденных проектов: {e}"))?; }
    notify(emit(&result, &current, if result.cancelled { "cancelled" } else { "done" }));
    control.phase.store(FINISHED, Ordering::SeqCst);
    Ok(result)
}

#[tauri::command]
pub(crate) async fn project_discovery_scan(state: tauri::State<'_, ProjectDiscoveryState>, id: String, root: String, on_progress: tauri::ipc::Channel<DiscoveryProgress>) -> Result<DiscoveryResult, String> {
    let operation = crate::app_updates::operation()?; let lease = state.reserve(id.clone())?; let control = lease.control.clone();
    tauri::async_runtime::spawn_blocking(move || { let _operation = operation; let _lease = lease; scan(Path::new(&root), id, &control, Options::default(), |p| { let _ = on_progress.send(p); }, crate::project_registry::register_discovered) }).await.map_err(|_| "Поиск проектов прерван.".to_string())?
}
#[tauri::command]
pub(crate) fn project_discovery_cancel(state: tauri::State<'_, ProjectDiscoveryState>, id: String) -> bool { state.cancel(&id) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_registry::tests::Fixture;
    fn put(root: &Path, relative: &str) -> PathBuf { let p = root.join(relative); fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(&p, br#"{"FileVersion":3,"EngineAssociation":"5.8"}"#).unwrap(); p }
    fn run(f: &Fixture, options: Options) -> DiscoveryResult { scan(&f.root.join("search"), uuid::Uuid::new_v4().to_string(), &Control::new(false), options, |_| {}, |paths| f.register_count(paths)).unwrap() }
    #[test]
    fn arbitrary_folder_names_are_searched_and_known_generated_project_subtrees_are_not() {
        let f = Fixture::new(); let search = f.root.join("search");
        put(&search, "Content/任意 空白/A.uproject"); put(&search, "Engine/Other/B.uproject"); put(&search, "AppData/Windows/C.uproject");
        put(&search, "real/Demo.uproject"); put(&search, "real/Intermediate/False.uproject"); put(&search, ".git/False.uproject");
        let first = run(&f, Options::default()); assert_eq!((first.found, first.added), (4, 4)); assert!(!first.limited); assert!(first.skipped >= 2);
        let bytes = fs::read(f.record()).unwrap(); let second = run(&f, Options::default()); assert_eq!((second.found, second.added), (4, 0)); assert_eq!(fs::read(f.record()).unwrap(), bytes);
        assert_eq!(crate::scan_projects_in(&[], &f.paths()).len(), 4);
    }
    #[test]
    fn cancel_preserves_found_prefix_and_old_registry_then_releases_single_job() {
        let f = Fixture::new(); let old = put(&f.root, "old/Original.uproject"); f.register(&[old.clone()]).unwrap();
        put(&f.root, "search/one/A.uproject"); put(&f.root, "search/two/B.uproject");
        let state = ProjectDiscoveryState::default(); let id = uuid::Uuid::new_v4().to_string(); let lease = state.reserve(id.clone()).unwrap();
        assert!(state.reserve(uuid::Uuid::new_v4().to_string()).is_err()); assert!(!state.cancel(&uuid::Uuid::new_v4().to_string()));
        let result = scan(&f.root.join("search"), id.clone(), &lease.control, Options { interval: Duration::ZERO, ..Default::default() }, |p| { if p.found == 1 && p.phase == "scan" { assert!(state.cancel(&id)); } if p.phase == "save" { assert!(!state.cancel(&id)); } }, |paths| f.register_count(paths)).unwrap();
        assert!(result.cancelled); assert_eq!((result.found, result.added), (1, 1)); assert_eq!(f.paths().len(), 2); assert!(f.paths().iter().any(|p| crate::project_registry::path_key(p) == crate::project_registry::path_key(&old)));
        drop(lease); assert!(state.reserve(uuid::Uuid::new_v4().to_string()).is_ok());
    }
    #[test]
    fn prestart_cancel_never_descends_and_no_result_does_not_create_registry() {
        let f = Fixture::new(); put(&f.root, "search/A.uproject"); let state = ProjectDiscoveryState::default(); let id = uuid::Uuid::new_v4().to_string();
        assert!(state.cancel(&id)); let lease = state.reserve(id.clone()).unwrap();
        let result = scan(&f.root.join("search"), id, &lease.control, Options::default(), |_| {}, |_| panic!("cancelled-empty must not register")).unwrap();
        assert!(result.cancelled); assert_eq!((result.directories, result.scanned_entries, result.found), (0, 0, 0)); assert!(!f.record().exists());
        drop(lease);
        for _ in 0..32 { assert!(state.cancel(&uuid::Uuid::new_v4().to_string())); }
        assert_eq!(state.jobs.lock().unwrap().cancelled_before_start.len(), 16);
    }
    #[test]
    fn depth_and_entry_limits_are_visible_and_do_not_discard_found_projects() {
        let f = Fixture::new(); put(&f.root, "search/Visible.uproject"); put(&f.root, "search/a/b/Deep.uproject");
        let depth = run(&f, Options { depth: 0, ..Default::default() }); assert!(depth.limited); assert_eq!(depth.limit_reason, Some("depth")); assert_eq!(depth.found, 1);
        let entries = run(&f, Options { entries: 0, ..Default::default() }); assert!(entries.limited); assert_eq!(entries.limit_reason, Some("entries")); assert_eq!(entries.scanned_entries, 0); assert_eq!(entries.added, 0);
        let timed = run(&f, Options { time: Duration::ZERO, ..Default::default() }); assert!(timed.limited); assert_eq!(timed.limit_reason, Some("time")); assert_eq!(timed.added, 0);
    }
    #[test]
    fn configured_roots_share_cancellation_and_save_partial_results_once() {
        let f = Fixture::new(); let old = put(&f.root, "known/Original.uproject"); f.register(&[old]).unwrap();
        let roots = [put(&f.root, "first/A.uproject").parent().unwrap().to_path_buf(), put(&f.root, "second/B.uproject").parent().unwrap().to_path_buf()];
        let state = ProjectDiscoveryState::default(); let id = uuid::Uuid::new_v4().to_string(); let lease = state.reserve(id.clone()).unwrap();
        let mut saves = 0; let mut terminal = Vec::new();
        let result = scan_checked(&roots, Vec::new(), id.clone(), &lease.control, Options { interval: Duration::ZERO, ..Default::default() }, |p| {
            if p.phase == "scan" && p.found == 1 { assert!(state.cancel(&id)); }
            if p.phase == "save" { assert!(!state.cancel(&id)); }
            terminal.push(p.phase);
        }, |paths| { saves += 1; f.register_count(paths) }).unwrap();
        assert!(result.cancelled); assert_eq!((result.found, result.added, result.directories), (1, 1, 1));
        assert_eq!(saves, 1); assert_eq!(terminal.iter().filter(|phase| **phase == "save").count(), 1); assert_eq!(terminal.last(), Some(&"cancelled"));
        assert_eq!(f.paths().len(), 2); drop(lease); assert!(state.reserve(uuid::Uuid::new_v4().to_string()).is_ok());
    }
    #[test]
    fn configured_roots_share_entry_budget_and_deduplicate_overlapping_findings() {
        let f = Fixture::new(); let first = put(&f.root, "first/A.uproject"); let second = put(&f.root, "second/B.uproject");
        let roots = [first.parent().unwrap().to_path_buf(), second.parent().unwrap().to_path_buf()];
        let limited = scan_checked(&roots, Vec::new(), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options { entries: 1, ..Default::default() }, |_| {}, |paths| f.register_count(paths)).unwrap();
        assert!(limited.limited); assert_eq!(limited.limit_reason, Some("entries")); assert_eq!((limited.scanned_entries, limited.found, limited.added), (1, 1, 1));
        let same = [roots[0].clone(), roots[0].clone()];
        let duplicated = scan_checked(&same, Vec::new(), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options::default(), |_| {}, |paths| f.register_count(paths)).unwrap();
        assert_eq!((duplicated.found, duplicated.added), (1, 0)); assert_eq!(f.paths().len(), 1);
    }
    #[test]
    fn malformed_descriptors_and_registry_failure_never_modify_original_project_files() {
        let f = Fixture::new(); let valid = put(&f.root, "search/Valid.uproject"); let invalid = put(&f.root, "search/Bad.uproject"); fs::write(&invalid, b"[]").unwrap();
        let before = fs::read(&valid).unwrap();
        let mut bad_path_observed = false;
        let inspected = scan(&f.root.join("search"), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options::default(), |_| {}, |paths| { assert_eq!(paths.len(), 1); assert_eq!(crate::project_registry::path_key(&paths[0]), crate::project_registry::path_key(&valid)); bad_path_observed = true; Ok(0) }).unwrap();
        let invalid_display = crate::project_registry::display_path(invalid.canonicalize().unwrap());
        let expected_context = format!("Не удалось проверить проект {}: ", invalid_display.display());
        assert!(bad_path_observed); assert!(inspected.errors.iter().any(|error| error.starts_with(&expected_context)), "expected exact descriptor context {expected_context:?}, got {:?}", inspected.errors);
        let error = scan(&f.root.join("search"), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options::default(), |_| {}, |_| Err("fixture write denied".into())).unwrap_err();
        assert!(error.starts_with("Не удалось сохранить список найденных проектов:")); assert_eq!(fs::read(valid).unwrap(), before); assert_eq!(fs::read(invalid).unwrap(), b"[]"); assert!(!f.record().exists());
        assert!(scan(Path::new("relative"), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options::default(), |_| {}, |_| panic!("invalid root")).is_err());
    }
    #[test]
    fn empty_directories_report_current_path_and_real_directory_counts() {
        let f = Fixture::new(); let search = f.root.join("search");
        for name in ["empty-a", "empty-b", "empty-c"] { fs::create_dir_all(search.join(name)).unwrap(); }
        let mut events = Vec::new();
        let result = scan(&search, uuid::Uuid::new_v4().to_string(), &Control::new(false), Options { interval: Duration::ZERO, ..Default::default() }, |p| events.push(p), |_| panic!("no descriptors to register")).unwrap();
        assert_eq!((result.directories, result.scanned_entries, result.found, result.added), (4, 3, 0, 0));
        for (index, name) in ["empty-a", "empty-b", "empty-c"].iter().enumerate() {
            assert!(events.iter().any(|event| event.phase == "scan" && Path::new(&event.current_directory).file_name().is_some_and(|value| value == *name) && event.directories == index + 1));
        }
        assert_eq!(events.last().unwrap().directories, 4); assert_eq!(events.last().unwrap().phase, "done"); assert!(!f.record().exists());
    }
    #[test]
    fn capacity_failure_preserves_every_saved_path_and_project_descriptor() {
        let f = Fixture::new(); let project = put(&f.root, "search/New.uproject");
        fs::create_dir_all(f.record().parent().unwrap()).unwrap();
        let paths: Vec<_> = (0..4096).map(|n| f.root.join(format!("unavailable-{n}/Old.uproject")).to_string_lossy().into_owned()).collect();
        let old = serde_json::to_vec(&serde_json::json!({"schema":1,"paths":paths})).unwrap(); fs::write(f.record(), &old).unwrap();
        let bytes = fs::read(&project).unwrap();
        assert!(scan(&f.root.join("search"), uuid::Uuid::new_v4().to_string(), &Control::new(false), Options::default(), |_| {}, |paths| f.register_count(paths)).unwrap_err().contains("4096"));
        assert_eq!(fs::read(f.record()).unwrap(), old); assert_eq!(fs::read(project).unwrap(), bytes);
    }
    #[cfg(windows)]
    #[test]
    fn junction_subtree_and_selected_junction_are_not_followed() {
        use std::os::windows::process::CommandExt;
        let f = Fixture::new(); let outside = f.root.join("outside"); put(&outside, "Private.uproject");
        let search = f.root.join("search"); fs::create_dir(&search).unwrap(); let link = search.join("linked");
        struct Junction(PathBuf); impl Drop for Junction { fn drop(&mut self) { if fs::symlink_metadata(&self.0).is_ok_and(|m| reparse(&m)) { let _ = fs::remove_dir(&self.0); } } }
        let command = format!("mklink /J \"{}\" \"{}\"", link.display(), outside.display());
        let junction = Junction(link.clone());
        let created = std::process::Command::new("cmd.exe").args(["/d", "/s", "/c"]).raw_arg(&command).creation_flags(0x08000000).output().unwrap();
        assert!(created.status.success(), "junction creation failed: status={}, stdout={:?}, stderr={:?}", created.status, String::from_utf8_lossy(&created.stdout), String::from_utf8_lossy(&created.stderr));
        let result = run(&f, Options::default()); assert_eq!((result.found, result.added), (0, 0)); assert_eq!(result.skipped, 1); assert!(!f.record().exists());
        assert!(checked_root(&link).is_err()); assert!(outside.join("Private.uproject").is_file()); drop(junction);
        assert_eq!(checked_root(Path::new(r"\\server\share\project")).unwrap_err(), NETWORK_ERROR);
    }
    #[test]
    fn worker_callback_panic_releases_job_without_registering_any_paths() {
        let f = Fixture::new(); put(&f.root, "search/A.uproject"); let state = ProjectDiscoveryState::default();
        let id = uuid::Uuid::new_v4().to_string(); let lease = state.reserve(id.clone()).unwrap(); let control = lease.control.clone();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lease = lease;
            let _ = scan(&f.root.join("search"), id, &control, Options { interval: Duration::ZERO, ..Default::default() }, |p| { if p.found > 0 { panic!("fixture progress consumer panic"); } }, |_| panic!("must not save after callback panic"));
        }));
        assert!(panic.is_err()); assert!(!f.record().exists()); assert!(state.reserve(uuid::Uuid::new_v4().to_string()).is_ok());
    }
}

use crate::vault::{hash_file, no_links, relative};
use egs_api::api::types::download_manifest::{DownloadManifest, FileManifestList};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
pub mod install;
pub mod delete;
mod manifest;
mod prerequisites;
mod scan_storage;
mod access;
pub mod resume;

#[derive(Default)]
pub struct EngineState {
    plan: Mutex<Option<Plan>>,
    pub active: AtomicBool,
    pub(crate) cancel: std::sync::Arc<AtomicBool>,
    pub(crate) paused: std::sync::Arc<AtomicBool>,
    pause_available: AtomicBool,
    pub(crate) suspend: std::sync::Arc<AtomicBool>,
    pub(crate) resumable: AtomicBool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRelease {
    app_name: String,
    version: String,
    build: String,
    installed: Vec<Installed>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    path: String,
    build: String,
    version: String,
    update_available: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    tag: String,
    label: String,
    bytes: u64,
    selected: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    app_name: String,
    build: String,
    path: String,
    update: bool,
    components: Vec<Component>,
    bytes: u64,
    download_bytes: u64,
    changed: usize,
    added: usize,
    removed: usize,
    required_bytes: u64,
    free_bytes: u64,
    conflicts: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Plan {
    #[serde(default)]
    resume_stage: Option<PathBuf>,
    catalog_item_id: Option<String>,
    remove_unselected: bool,
    source_base: String,
    info: Preview,
    raw: Vec<u8>,
    #[serde(with="resume::snapshot_files")]
    files: BTreeMap<String, FileManifestList>,
    #[serde(with="resume::snapshot_files")]
    old: BTreeMap<String, FileManifestList>,
    item: Option<(PathBuf, serde_json::Value)>,
    chunk_sizes: BTreeMap<String, u64>,
    tags: BTreeMap<String, Vec<String>>,
}
fn io(e: std::io::Error) -> String {
    format!("Ошибка файловой системы: {e}")
}
pub(crate) fn parse(raw: &[u8]) -> Result<DownloadManifest, String> {
    manifest::tags(raw)?;
    let (app, build) = manifest::identity(raw)?;
    let mut m = std::panic::catch_unwind(|| DownloadManifest::parse(raw.to_vec()))
        .ok()
        .flatten()
        .ok_or("Не удалось прочитать манифест движка")?;
    if m.app_name_string != app {
        return Err("Повреждена идентификация движка".into());
    }
    // egs-api 0.14 overwrites BuildVersion with BuildId in v1 metadata.
    m.build_version_string = build;
    Ok(m)
}

fn same_installation_path(a:&str,b:&str)->bool {
    fn normalized(p:&str)->String {let p=p.replace('/',"\\");let unc=p.starts_with("\\\\");let parts=p.split('\\').filter(|s|!s.is_empty()).collect::<Vec<_>>().join("\\");if unc{format!("\\\\{parts}")}else{parts}}
    normalized(a).eq_ignore_ascii_case(&normalized(b))
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn epic_and_scanner_paths_match_separators_case_and_trailing_slash() {
        assert!(super::same_installation_path(r"X:\Fixture\Engines\UE_5.8", "x:/Fixture/Engines/UE_5.8/"));
        assert!(!super::same_installation_path(r"X:\Fixture\Engines\UE_5.8", "X:/Fixture/Engines/UE_5.7"));
        assert!(super::same_installation_path(r"X:\\Fixture\\Engines\\UE_5.8", "X:/Fixture/Engines/UE_5.8"));
    }
}

fn items() -> Vec<(PathBuf, serde_json::Value)> {
    let root =
        PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into()))
            .join("Epic/EpicGamesLauncher/Data/Manifests");
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()?.to_str()? != "item" {
                return None;
            }
            let v: serde_json::Value = serde_json::from_slice(&fs::read(&p).ok()?).ok()?;
            if v["CatalogNamespace"] != "ue" || !v["AppName"].as_str()?.starts_with("UE_") {
                return None;
            }
            Some((p, v))
        })
        .collect()
}
fn version(root: &Path) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("Engine/Build/Build.version")).map_err(io)?)
            .map_err(|_| "Не удалось прочитать версию движка")?;
    let n = |k: &str| v[k].as_u64().ok_or("Некорректная версия движка");
    Ok((
        format!(
            "{}.{}.{}",
            n("MajorVersion")?,
            n("MinorVersion")?,
            n("PatchVersion")?
        ),
        n("Changelist")?.to_string(),
    ))
}
fn newer(current: &str, latest: &str) -> bool {
    let cl = |s: &str| {
        s.split('-')
            .nth(1)
            .and_then(|s| s.split('+').next())
            .and_then(|s| s.parse::<u64>().ok())
    };
    matches!((cl(current),cl(latest)),(Some(a),Some(b)) if b>a)
}
fn cached_catalog(path: &Path) -> Vec<EngineRelease> {
    fs::read(path).ok().and_then(|raw| serde_json::from_slice::<Vec<EngineRelease>>(&raw).ok()).unwrap_or_default()
}
fn save_catalog(path: &Path, releases: &[EngineRelease]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Некорректный путь кэша")?).map_err(io)?;
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&tmp, serde_json::to_vec(releases).map_err(|e| e.to_string())?).map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
}
#[tauri::command]
pub async fn engine_catalog(
    app: tauri::AppHandle,
    epic: tauri::State<'_, crate::epic::EpicState>,
    cached_only: Option<bool>,
) -> Result<Vec<EngineRelease>, String> {
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let cache = &crate::storage::cache().join("engines.json");
    let mut releases = if cached_only.unwrap_or(false) {
        cached_catalog(cache)
    } else {
        let mut fresh = Vec::new();
        for a in crate::epic::engine_assets(&app, &epic).await? {
            fresh.push(EngineRelease {
                version: a.app_name.trim_start_matches("UE_").into(),
                app_name: a.app_name,
                build: a.build_version,
                installed: Vec::new(),
            });
        }
        // Only public catalogue information is persisted; installation state is read locally.
        let _ = save_catalog(cache, &fresh);
        fresh
    };
    let local = crate::scan_engines();
    let items = items();
    for release in &mut releases {
        release.installed = local.iter().filter(|e| e.version == release.version).filter_map(|e| {
            let (v, cl) = version(Path::new(&e.path)).ok()?;
            let build = items.iter().find(|(_, i)| i["InstallLocation"].as_str().is_some_and(|p| same_installation_path(p,&e.path)))
                .and_then(|(_, i)| i["AppVersionString"].as_str()).map(str::to_owned).unwrap_or(format!("{v}-{cl}"));
            Some(Installed {path:e.path.clone(),version:v,update_available:newer(&build,&release.build),build})
        }).collect();
    }
    releases.sort_by_key(|a| {
        let mut n = a.version.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
        std::cmp::Reverse((n.next().unwrap_or(0), n.next().unwrap_or(0)))
    });
    Ok(releases)
}

fn label(tag: &str) -> String {
    match tag {
        "editor_symbols" => "Отладочные символы",
        "engine_source" => "Исходный код",
        "templates" => "Шаблоны проектов",
        "metahuman_content" => "MetaHuman",
        _ => tag.strip_prefix("platform_").unwrap_or(tag),
    }
    .into()
}
fn target(path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(path);
    if !root.is_absolute()
        || root.parent().is_none()
        || root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Выберите полный путь к папке движка".into());
    }
    no_links(&root)?;
    Ok(root)
}
#[cfg(windows)]
pub(crate) fn free(root: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    let mut p = root;
    while !p.exists() {
        p = p.parent().ok_or("Диск не найден")?;
    }
    let s: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    let mut available = 0u64;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            s.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err("Не удалось проверить свободное место".into())
    } else {
        Ok(available)
    }
}
#[cfg(not(windows))]
pub(crate) fn free(_: &Path) -> Result<u64, String> {
    Err("Поддерживается Windows".into())
}
#[tauri::command]
pub async fn engine_prepare(
    app: tauri::AppHandle,
    epic: tauri::State<'_, crate::epic::EpicState>,
    state: tauri::State<'_, EngineState>,
    vault: tauri::State<'_, crate::vault::VaultState>,
    app_name: String,
    path: String,
    selected: Option<Vec<String>>,
    update: bool,
    modify: Option<bool>,
) -> Result<Preview, String> {
    let _update_operation=crate::app_updates::operation()?;
    if state.active.load(Ordering::SeqCst) {
        return Err("Дождитесь текущей операции с движком".into());
    }
    if vault.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции с хранилищем".into());}
    let _vault=install::Busy(&vault.active);
    let modify = modify.unwrap_or(false);
    if modify && !update { return Err("Установленный движок не найден. Повторите сканирование.".into()); }
    let root = target(&path)?;
    let item = items().into_iter().find(|(_, v)| {
        v["InstallLocation"]
            .as_str()
            .is_some_and(|p| same_installation_path(p,&path))
    });
    let installed = root.join("Engine/Build/Build.version").is_file();
    if installed != update {
        return Err(if update {
            "Установленный движок не найден. Повторите сканирование."
        } else {
            "Для новой установки выберите отдельную пустую папку."
        }
        .into());
    }
    if update {
        let (v, _) = version(&root)?;
        if !v.starts_with(&format!("{}.", app_name.trim_start_matches("UE_"))) {
            return Err("Новая номерная версия устанавливается в отдельную папку".into());
        }
        if item.as_ref().is_some_and(|(_, i)| i["AppName"] != app_name)
            || item.is_none() && !root.join(".egstore/um-install.json").is_file()
        {
            return Err(
                "Для обновления нужен исходный манифест установки Epic или Unreal Manager".into(),
            );
        }
    } else if root.exists() && fs::read_dir(&root).map_err(io)?.next().is_some() {
        return Err("Для новой версии выберите пустую папку".into());
    }
    let cached = if selected.is_some() {
        state
            .plan
            .lock()
            .map_err(|_| "Ошибка состояния")?
            .as_ref()
            .filter(|p| p.info.app_name == app_name && p.info.path.eq_ignore_ascii_case(&path))
            .map(|p| (p.raw.clone(), p.source_base.clone(),p.catalog_item_id.clone()))
    } else {
        None
    };
    let (raw, base,catalog_item_id) = match cached {
        Some(v) => v,
        None => crate::epic::engine_manifest(&app, &epic, &app_name).await?,
    };
    let plan = tauri::async_runtime::spawn_blocking(move || {
        if modify {
            let (_, build) = manifest::identity(&raw)?;
            let (installed_version, changelist) = version(&root)?;
            if !same_build(&build, &installed_version, &changelist) {
                return Err("Для изменения компонентов сначала обновите движок до доступной сборки.".into());
            }
        }
        let mut plan = prepare(root.clone(), item, raw, base, app_name, selected)?;
        plan.catalog_item_id=catalog_item_id;
        plan.remove_unselected = modify;
        if modify {
            plan.info.removed = plan.old.keys().filter(|n| !plan.files.contains_key(*n) && root.join(n).is_file()).count();
        }
        Ok::<_, String>(plan)
    })
    .await
    .map_err(|_| "Подготовка прервана")??;
    let info = plan.info.clone();
    *state.plan.lock().map_err(|_| "Ошибка состояния")? = Some(plan);
    Ok(info)
}
fn same_build(build: &str, version: &str, changelist: &str) -> bool {
    build.split_once('-').is_some_and(|(v, rest)| v == version && rest.split('+').next() == Some(changelist))
}
fn prepare(
    root: PathBuf,
    item: Option<(PathBuf, serde_json::Value)>,
    raw: Vec<u8>,
    base: String,
    app_name: String,
    selected: Option<Vec<String>>,
) -> Result<Plan, String> {
    let tags = manifest::tags(&raw)?;
    let mut m = parse(&raw)?;
    if m.app_name_string != app_name || m.b_is_file_data {
        return Err("Неподходящий манифест движка".into());
    }
    m.custom_fields
        .get_or_insert_with(Default::default)
        .insert("SourceURL".into(), base.trim_end_matches('/').into());
    let own = if root.join(".egstore/um-install.json").is_file() {
        let p = root.join(".egstore/um-install.json");
        no_links(&p)?;
        Some(
            serde_json::from_slice::<serde_json::Value>(&fs::read(p).map_err(io)?)
                .map_err(|_| "Повреждена запись установки")?,
        )
    } else {
        None
    };
    let old_path = item
        .as_ref()
        .and_then(|(_, v)| v["CompleteManifestPath"].as_str())
        .or_else(|| own.as_ref().and_then(|v| v["manifest"].as_str()));
    let mut installed_tags = None;
    let old = if let Some(p) = old_path {
        let p = PathBuf::from(p);
        no_links(&p)?;
        let bytes = fs::read(p).map_err(io)?;
        let old = parse(&bytes)?;
        if let Some((_, ref item)) = item {
            installed_tags = Some(infer_components(
                &old,
                &manifest::tags(&bytes)?,
                item["InstallSize"]
                    .as_u64()
                    .ok_or("Нет размера установки Epic")?,
            )?);
        }
        if old.app_name_string != app_name {
            return Err("Исходный манифест другой версии".into());
        }
        if newer(&m.build_version_string, &old.build_version_string) {
            return Err("Установленная сборка новее доступной. Понижение версии запрещено.".into());
        }
        old.file_manifest_list
            .into_iter()
            .map(|f| (f.filename.clone(), f))
            .collect()
    } else {
        BTreeMap::<String, FileManifestList>::new()
    };
    let mut components: BTreeMap<String, Component> = BTreeMap::new();
    let files: BTreeMap<_, _> = m.files().into_iter().collect();
    let mut seen = BTreeSet::new();
    if files.len() != m.file_manifest_list.len() || files.len() != tags.len() {
        return Err("Неполный манифест движка".into());
    }
    for original in &m.file_manifest_list {
        if files
            .get(&original.filename)
            .is_none_or(|f| f.file_chunk_parts.len() != original.file_chunk_parts.len())
        {
            return Err("Нет адресов всех блоков движка".into());
        }
    }
    for (name, f) in &files {
        relative(name)?;
        if !seen.insert(name.to_lowercase())
            || f.file_hash.len() != 40
            || !f.file_hash.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Некорректный список файлов".into());
        }
        for part in &f.file_chunk_parts {
            if part.link.as_ref().is_none_or(|u| u.scheme() != "https")
                || part.offset + part.size > 64 * 1024 * 1024
            {
                return Err("Некорректный блок движка".into());
            }
        }
        for tag in tags.get(name).ok_or("Неполный список компонентов")? {
            let c = components.entry(tag.clone()).or_insert(Component {
                tag: tag.clone(),
                label: label(tag),
                bytes: 0,
                selected: false,
            });
            c.bytes += f.size() as u64;
        }
    }
    if let Some(ref selection) = selected {
        for t in selection {
            if !components.contains_key(t) {
                return Err("Компонент не найден в манифесте".into());
            }
        }
        for c in components.values_mut() {
            c.selected = selection.contains(&c.tag);
        }
    } else if let Some(ref own) = own {
        let saved = own["tags"]
            .as_array()
            .ok_or("Нет списка установленных компонентов")?;
        for c in components.values_mut() {
            c.selected = saved.iter().any(|t| t.as_str() == Some(&c.tag));
        }
    } else if let Some(ref tags) = installed_tags {
        for c in components.values_mut() {
            c.selected = tags.contains(&c.tag);
        }
    } else if old.is_empty() {
        for c in components.values_mut() {
            c.selected = matches!(c.tag.as_str(), "engine_source" | "templates");
        }
    }
    let chosen: BTreeSet<_> = components
        .values()
        .filter(|c| c.selected)
        .map(|c| c.tag.clone())
        .collect();
    let mut changed = 0;
    let mut added = 0;
    let mut bytes = 0;
    let mut chunks = BTreeMap::new();
    let conflicts = Vec::new();
    let mut selected_files = BTreeMap::new();
    for (name, f) in files {
        let ts = &tags[&name];
        if !ts.is_empty() && !ts.iter().any(|t| chosen.contains(t)) {
            continue;
        }
        let current = old.get(&name);
        let different = current.is_none_or(|o| o.file_hash != f.file_hash) || !root.join(&name).is_file();
        if different {
            if current.is_some() {
                changed += 1
            } else {
                added += 1
            };
            bytes += f.size() as u64;
            for part in &f.file_chunk_parts {
                chunks.insert(
                    part.guid.clone(),
                    m.chunk_filesize_list
                        .get(&part.guid)
                        .copied()
                        .unwrap_or(64 * 1024 * 1024) as u64,
                );
            }
        }
        selected_files.insert(name, f);
    }
    let removed = old
        .keys()
        .filter(|n| !tags.contains_key(*n) && root.join(n).is_file())
        .count();
    let download_bytes = chunks.values().sum();
    let required_bytes = bytes + download_bytes + 1024 * 1024 * 1024;
    let free_bytes = free(&root)?;
    let info = Preview {
        id: uuid::Uuid::new_v4().to_string(),
        app_name,
        build: m.build_version_string,
        path: root.to_string_lossy().into_owned(),
        update: !old.is_empty(),
        components: components.into_values().collect(),
        bytes,
        download_bytes,
        changed,
        added,
        removed,
        required_bytes,
        free_bytes,
        conflicts,
    };
    Ok(Plan {
        resume_stage: None,
        catalog_item_id: None,
        remove_unselected: false,
        source_base: base,
        info,
        raw,
        files: selected_files,
        old,
        item,
        chunk_sizes: m
            .chunk_filesize_list
            .into_iter()
            .map(|(g, n)| (g, n as u64))
            .collect(),
        tags,
    })
}
#[tauri::command]
pub fn engine_launch(path: String, state: tauri::State<'_, EngineState>) -> Result<(), String> {
    if state.active.load(Ordering::SeqCst) {
        return Err("Дождитесь завершения установки движка".into());
    }
    let root = target(&path)?;
    version(&root)?;
    if root.join(".um-engine-update.json").exists() {
        return Err("Предыдущее обновление не завершено. Сначала восстановите установку.".into());
    }
    std::process::Command::new(root.join("Engine/Binaries/Win64/UnrealEditor.exe"))
        .current_dir(root)
        .spawn()
        .map_err(io)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn catalogue_cache_roundtrip_replace_and_corruption() {
        let root = std::env::temp_dir().join(format!("um-catalog-test-{}", uuid::Uuid::new_v4()));
        let path = root.join("engines.json");
        assert!(super::cached_catalog(&path).is_empty());
        let mut entries = vec![super::EngineRelease {app_name:"UE_5.8".into(),version:"5.8".into(),build:"5.8.3-1".into(),installed:vec![]}];
        super::save_catalog(&path,&entries).unwrap();
        assert_eq!(super::cached_catalog(&path)[0].build,"5.8.3-1");
        entries[0].build="5.8.4-2".into();
        super::save_catalog(&path,&entries).unwrap();
        assert_eq!(super::cached_catalog(&path)[0].build,"5.8.4-2");
        std::fs::write(&path,b"broken cache").unwrap();
        assert!(super::cached_catalog(&path).is_empty());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
    use super::*;
    #[test]
    fn build_comparison() {
        assert!(newer("5.8.1-56057345+++UE5", "5.8.3-58210709+++UE5"));
        assert!(!newer("5.8.3-58210709+++UE5", "5.8.3-58210709+++UE5"));
        assert!(!newer("unknown", "new"));
    }
}

pub(crate) fn local_report(raw_path: &Path, root: PathBuf) -> Result<Preview, String> {
    let item = items().into_iter().find(|(_, v)| v["AppName"] == "UE_5.8");
    Ok(prepare(
        root,
        item,
        fs::read(raw_path).map_err(io)?,
        "https://egdownload.fastly-edge.com/Builds/UE5/Releases/CloudDir/".into(),
        "UE_5.8".into(),
        None,
    )?
    .info)
}

pub(crate) fn managed_roots() -> Vec<PathBuf> {
    let base = crate::storage::settings();
    fs::read(base.join("engine-installs.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<PathBuf>>(&b).ok())
        .unwrap_or_default()
}
fn register_root(root: &Path) -> Result<(), String> {
    let base = crate::storage::settings();
    no_links(&base)?;
    fs::create_dir_all(&base).map_err(io)?;
    let mut roots = managed_roots();
    if !roots.iter().any(|p| {
        p.to_string_lossy()
            .eq_ignore_ascii_case(&root.to_string_lossy())
    }) {
        roots.push(root.to_path_buf());
    }
    let temp = base.join(format!("engine-installs-{}.json", uuid::Uuid::new_v4()));
    fs::write(&temp, serde_json::to_vec_pretty(&roots).unwrap()).map_err(io)?;
    fs::rename(temp, base.join("engine-installs.json")).map_err(io)
}

#[cfg(any(debug_assertions, feature = "engine-test-cli"))]
pub(crate) async fn install_local(
    app: &tauri::AppHandle,
    raw_path: PathBuf,
    root: PathBuf,
    report: PathBuf,
) -> Result<String, String> {
    use tauri::Manager;
    let item = items().into_iter().find(|(_, v)| v["AppName"] == "UE_5.8");
    let plan = tauri::async_runtime::spawn_blocking(move || {
        prepare(
            root,
            item,
            fs::read(raw_path).map_err(io)?,
            "https://egdownload.fastly-edge.com/Builds/UE5/Releases/CloudDir".into(),
            "UE_5.8".into(),
            None,
        )
    })
    .await
    .map_err(|_| "Подготовка прервана")??;
    fs::write(
        report.with_extension("plan.json"),
        serde_json::to_vec_pretty(&plan.info).unwrap(),
    )
    .map_err(io)?;
    let id = plan.info.id.clone();
    *app.state::<EngineState>()
        .plan
        .lock()
        .map_err(|_| "Ошибка состояния")? = Some(plan);
    let channel = tauri::ipc::Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            let _ = fs::write(&report, json);
        }
        Ok(())
    });
    install::engine_install(
        app.clone(),
        app.state::<crate::epic::EpicState>(),
        app.state::<EngineState>(),
        app.state::<crate::vault::VaultState>(),
        id,
        channel,
    )
    .await
}

fn infer_components(
    m: &DownloadManifest,
    tags: &BTreeMap<String, Vec<String>>,
    installed_size: u64,
) -> Result<BTreeSet<String>, String> {
    let mut groups: BTreeMap<Vec<String>, u64> = BTreeMap::new();
    for f in &m.file_manifest_list {
        let mut group = tags
            .get(&f.filename)
            .ok_or("Нет компонентов файла")?
            .clone();
        group.sort();
        *groups.entry(group).or_default() += f.size() as u64;
    }
    selection_for_size(&groups, installed_size)
}
fn selection_for_size(
    groups: &BTreeMap<Vec<String>, u64>,
    installed_size: u64,
) -> Result<BTreeSet<String>, String> {
    let names: Vec<_> = groups
        .keys()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if names.len() > 16 {
        return Err("Неизвестная схема компонентов Epic".into());
    }
    let mut found = None;
    for mask in 0..(1u32 << names.len()) {
        let selected: BTreeSet<_> = names
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, t)| t.clone())
            .collect();
        let size: u64 = groups
            .iter()
            .filter(|(g, _)| g.is_empty() || g.iter().any(|t| selected.contains(t)))
            .map(|(_, n)| *n)
            .sum();
        if size == installed_size {
            if found.is_some() {
                return Err("Состав установки Epic неоднозначен. Обновление не начато.".into());
            }
            found = Some(selected);
        }
    }
    found.ok_or("Не удалось подтвердить состав установки Epic по исходному манифесту".into())
}
#[cfg(test)]
mod component_tests {
    use super::*;
    #[test]
    fn shared_source_files_do_not_select_extra_platform() {
        let groups = BTreeMap::from([
            (vec![], 100),
            (vec!["source".into()], 10),
            (vec!["source".into(), "Android".into()], 2),
            (vec!["Android".into()], 50),
        ]);
        assert_eq!(
            selection_for_size(&groups, 112).unwrap(),
            BTreeSet::from(["source".into()])
        );
        assert!(selection_for_size(&groups, 111).is_err());
    }
    #[test]
    fn ambiguous_selection_is_rejected() {
        let groups = BTreeMap::from([
            (vec![], 100),
            (vec!["A".into()], 10),
            (vec!["B".into()], 10),
        ]);
        assert!(selection_for_size(&groups, 110).is_err());
    }
}

#[tauri::command]
pub fn engine_discard(state: tauri::State<'_, EngineState>) {
    if !state.active.load(Ordering::SeqCst) {
        if let Ok(mut p) = state.plan.lock() {
            *p = None;
        }
    }
}

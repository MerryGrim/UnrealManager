mod ui_locale;
mod storage;
mod download;
mod window_behavior;
mod webview_policy;
mod project_association;
mod project_registry;
mod project_discovery;
mod project_scan_folders;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;
mod engine;
mod epic;
mod epic_login;
mod epic_redeem;
mod epic_redeem_batch;
mod epic_browser;
mod vault;
mod vault_queue;
mod fab_cache;
mod fab_details;
mod project_actions;
mod app_updates;
mod app_update_apply;
mod elevation;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EngineInstall {
    name: String,
    version: String,
    path: String,
    source: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UProjectDescriptor {
    #[serde(default, rename = "EngineAssociation")]
    engine_association: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UnrealProject {
    name: String,
    path: String,
    engine_association: Option<String>,
    modified_unix: u64,
    thumbnail: Option<String>,
}

fn project_thumbnail(path: &Path) -> Option<String> {
    use base64::Engine;
    use std::io::{Cursor, Read};
    let root=path.parent()?;
    let candidates=[path.with_extension("png"), root.join("Saved/AutoScreenshot.png"), root.join("Thumbnail.png"), path.with_extension("jpg"), root.join("Build/Windows/Application.ico")];
    for candidate in candidates {
        let Ok(file)=fs::File::open(&candidate) else {continue};
        let mut bytes=Vec::new();
        if file.take(4*1024*1024+1).read_to_end(&mut bytes).is_err() || bytes.len()>4*1024*1024 {continue;}
        if candidate.extension().and_then(|s|s.to_str())==Some("ico") {
            if bytes.starts_with(&[0,0,1,0]) {return Some(format!("data:image/x-icon;base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes)));}
            continue;
        }
        let Ok(mut reader)=image::ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {continue};
        let mut limits=image::Limits::default();
        limits.max_image_width=Some(8192);limits.max_image_height=Some(8192);limits.max_alloc=Some(64*1024*1024);
        reader.limits(limits);
        let Ok(decoded)=reader.decode() else {continue};
        let mut output=Cursor::new(Vec::new());
        if decoded.thumbnail(384,216).write_to(&mut output,image::ImageFormat::Png).is_ok() {
            return Some(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(output.into_inner())));
        }
    }
    None
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn engine_version_from_dir(path: &Path) -> String {
    if let Ok(bytes)=fs::read(path.join("Engine/Build/Build.version")) {
        if let Ok(value)=serde_json::from_slice::<serde_json::Value>(&bytes) {
            if let (Some(major),Some(minor))=(value["MajorVersion"].as_u64(),value["MinorVersion"].as_u64()){return format!("{major}.{minor}");}
        }
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unreal Engine")
        .trim_start_matches("UE_")
        .to_string()
}

fn is_engine_root(path: &Path) -> bool {
    let bin = path.join("Engine/Binaries/Win64");
    bin.join("UnrealEditor.exe").is_file() || bin.join("UE4Editor.exe").is_file()
}

fn manifest_engine_paths() -> Vec<PathBuf> {
    let base = std::env::var_os("PROGRAMDATA").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let raw = fs::read_to_string(base.join("Epic/UnrealEngineLauncher/LauncherInstalled.dat"))
        .unwrap_or_default();
    let data: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    data["InstallationList"].as_array().into_iter().flatten()
        .filter_map(|item| item["InstallLocation"].as_str().map(PathBuf::from)).collect()
}

#[cfg(windows)]
fn registry_engine_paths() -> Vec<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;

    let mut out = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let keys = [
        r"SOFTWARE\EpicGames\Unreal Engine",
        r"SOFTWARE\WOW6432Node\EpicGames\Unreal Engine",
    ];

    for key_name in keys {
        if let Ok(key) = hklm.open_subkey(key_name) {
            for version in key.enum_keys().flatten() {
                if let Ok(sub) = key.open_subkey(&version) {
                    let installed: Result<String, _> = sub.get_value("InstalledDirectory");
                    if let Ok(p) = installed {
                        out.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(r"Software\Epic Games\Unreal Engine\Builds") {
        for (name, _) in key.enum_values().flatten() {
            if let Ok(value) = key.get_value::<String, _>(&name) { out.push(PathBuf::from(value)); }
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_engine_paths() -> Vec<PathBuf> { Vec::new() }

fn common_engine_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from(r"C:\Program Files\Epic Games"),
        PathBuf::from(r"C:\Epic Games"),
        PathBuf::from(r"D:\Epic Games"),
        PathBuf::from(r"D:\Unreal Engine"),
        PathBuf::from(r"E:\Epic Games"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("UnrealEngine"));
    }
    roots
}

#[tauri::command]
fn scan_engines() -> Vec<EngineInstall> {
    let mut candidates = registry_engine_paths();
    candidates.extend(manifest_engine_paths());
    candidates.extend(engine::managed_roots());
    for root in common_engine_roots() {
        if !root.exists() { continue; }
        if is_engine_root(&root) {
            candidates.push(root);
            continue;
        }
        if let Ok(entries) = fs::read_dir(root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && is_engine_root(&p) {
                    candidates.push(p);
                }
            }
        }
    }

    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for path in candidates {
        if !is_engine_root(&path) || path.join(".um-engine-update.json").exists() { continue; }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let key = canonical.to_string_lossy().to_lowercase();
        if !seen.insert(key) { continue; }
        result.push(EngineInstall {
            name: path.file_name().and_then(|s| s.to_str()).unwrap_or("Unreal Engine").to_string(),
            version: engine_version_from_dir(&path),
            path: path_string(&path),
            source: "Local installation".to_string(),
        });
    }
    result.sort_by(|a,b| b.version.cmp(&a.version));
    result
}

fn project_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Documents").join("Unreal Projects"));
        roots.push(home.join("Documents").join("UnrealProjects"));
    }
    roots.extend([
        PathBuf::from(r"D:\Unreal Projects"),
        PathBuf::from(r"D:\Projects"),
        PathBuf::from(r"E:\Unreal Projects"),
    ]);
    roots
}

fn read_project(path: &Path) -> Option<UnrealProject> {
    let raw = fs::read_to_string(path).ok()?;
    let descriptor: UProjectDescriptor = serde_json::from_str(&raw).unwrap_or(UProjectDescriptor { engine_association: None });
    let name = path.file_stem()?.to_string_lossy().to_string();
    let modified_unix = fs::metadata(path).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs()).unwrap_or(0);
    Some(UnrealProject {
        name,
        path: path_string(path),
        engine_association: descriptor.engine_association,
        modified_unix,
        thumbnail: project_thumbnail(path),
    })
}

fn scan_projects_in(roots: &[PathBuf], registered: &[PathBuf]) -> Vec<UnrealProject> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();

    for root in roots {
        if !root.exists() { continue; }
        for entry in WalkDir::new(root)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()).map(|x| x.eq_ignore_ascii_case("uproject")).unwrap_or(false) {
                let key = project_registry::path_key(path);
                if seen.insert(key) {
                    if let Some(project) = read_project(path) { result.push(project); }
                }
            }
        }
    }
    for path in registered {
        if seen.insert(project_registry::path_key(path)) {
            if let Some(project) = read_project(path) { result.push(project); }
        }
    }
    result.sort_by(|a,b| b.modified_unix.cmp(&a.modified_unix));
    result
}

fn scan_projects_local() -> Vec<UnrealProject> {
    scan_projects_in(&project_roots(), &project_registry::paths().unwrap_or_default())
}

#[tauri::command]
async fn scan_projects(state: tauri::State<'_, project_discovery::ProjectDiscoveryState>) -> Result<Vec<UnrealProject>, String> {
    let operation = app_updates::operation()?;
    let scan = state.reserve(uuid::Uuid::new_v4().to_string())?;
    tauri::async_runtime::spawn_blocking(move || { let _operation = operation; let _scan = scan; scan_projects_local() }).await.map_err(|_| "Поиск проектов прерван.".to_string())
}

#[tauri::command]
fn launch_project(path: String) -> Result<(), String> {
    let project = PathBuf::from(&path);
    if !project.is_file() || project.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("uproject")) || path.contains('\0') { return Err("Файл проекта .uproject не найден".into()); }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let file:Vec<u16>=project.as_os_str().encode_wide().chain([0]).collect();
        let verb:Vec<u16>="open".encode_utf16().chain([0]).collect();
        let result=unsafe{windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(),verb.as_ptr(),file.as_ptr(),std::ptr::null(),std::ptr::null(),1)} as isize;
        if result<=32{return Err(format!("Windows не смогла открыть проект (код {result})"));}
        return Ok(());
    }

    #[cfg(not(windows))]
    { Err("Project launch is implemented for Windows in this alpha".into()) }
}

pub fn run() {
    use tauri::Manager;
    let mut context=tauri::generate_context!();
    // Config paths must be relative in Tauri; the builder accepts our resolved absolute path.
    for window in &mut context.config_mut().app.windows {window.create=false;window.data_directory=None;}
    let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![app_update_apply::app_update_ui_ready,app_updates::app_update_settings_get,app_updates::app_update_settings_set,app_updates::app_update_check,app_updates::app_update_download,app_updates::app_update_apply,window_behavior::close_behavior_get,window_behavior::close_behavior_set,window_behavior::window_close_action,download::download_limit_get, download::download_limit_set,project_association::project_association_get,project_association::project_association_set,storage::vault_storage_get,storage::vault_storage_set,storage::settings_read,storage::settings_write,storage::storage_info,storage::clear_cache,ui_locale::set_ui_language,open_author_profile,engine::install::engine_register_managed,engine::delete::engine_delete_preview, engine::delete::engine_delete, engine::engine_discard, engine::resume::engine_resume_forget, engine::resume::engine_resume_tasks, engine::resume::engine_resume_prepare, engine::resume::engine_resume_legacy_prepare, engine::install::engine_pending, engine::install::engine_recover, engine::install::engine_install, engine::install::engine_cancel, engine::install::engine_pause, engine::engine_catalog, engine::engine_prepare, engine::engine_launch, scan_engines, scan_projects, launch_project, project_actions::reveal_project, project_actions::choose_project_location, project_actions::project_delete_preview, project_actions::delete_project,
        project_discovery::project_discovery_scan,project_discovery::project_discovery_cancel,
        project_scan_folders::list_scan_folders,project_scan_folders::set_scan_folders,project_scan_folders::refresh_projects,
        epic::epic_status, epic::epic_begin_login, epic_login::epic_cancel_login,
        epic::epic_logout, epic_redeem::epic_open_redeem, epic_redeem_batch::epic_redeem_batch_prepare, epic_redeem_batch::epic_redeem_batch_activate, epic_redeem_batch::epic_redeem_batch_cancel, epic_redeem_batch::epic_redeem_batch_discard, epic::fab_library, epic::fab_cached_library, epic::fab_cover,
        fab_details::fab_details, fab_details::fab_detail_image, fab_details::fab_open_listing,
        vault_queue::vault_queue_get,vault_queue::vault_queue_enqueue,vault_queue::vault_queue_remove,vault_queue::vault_queue_reorder,vault_queue::vault_queue_set_paused,vault_queue::vault_queue_retry,vault::vault_discard,vault::transfers::vault_pause,vault::transfers::vault_resume_tasks,vault::transfers::vault_resume_prepare,vault::transfers::vault_resume_forget,vault::project_update::project_update_preview, vault::project_update::project_update_apply, vault::vault_epic_status, vault::vault_sync_epic, vault::vault_check_update, vault::vault_prepare, vault::vault_download, vault::vault_cancel, vault::vault_list, vault::vault_reveal, vault::vault_delete_preview, vault::vault_delete, vault::plugins_installed, vault::plugin_remove_engine, vault::vault_install_preview, vault::vault_prepared_preview, vault::vault_install];
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            project_association::handle_args(app,&args);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            storage::initialize().map_err(std::io::Error::other)?;
            let config=app.config().app.windows.first().ok_or("Main window configuration missing")?;
            // A cosmetic preference must not prevent showing the recoverable settings error UI.
            let theme=storage::interface_theme().unwrap_or(tauri::Theme::Dark);
            let background=if theme==tauri::Theme::Light {tauri::window::Color(243,246,250,255)} else {tauri::window::Color(16,19,24,255)};
            tauri::WebviewWindowBuilder::from_config(app,config)?
                .theme(Some(theme)).background_color(background)
                .additional_browser_args(crate::webview_policy::BROWSER_ARGUMENTS)
                .data_directory(storage::root().join("WebView")).build()?;
            window_behavior::setup(app.handle())?;
            vault_queue::setup(app.handle());
            project_association::handle_args(app.handle(),&std::env::args().collect::<Vec<_>>());
            Ok(())
        })
        .manage(epic::EpicState::default())
        .manage(vault::VaultState::default())
        .manage(engine::EngineState::default())
        .manage(project_discovery::ProjectDiscoveryState::default())
        .on_window_event(|window,event| {
            if window.label()=="main" {
                if let tauri::WindowEvent::CloseRequested{api,..}=event {
                    api.prevent_close();
                    window_behavior::request_close(tauri::Manager::app_handle(window));
                }
            }
        })
        .invoke_handler(move |invoke| {
            if app_updates::applying() {
                invoke.resolver.reject("Дождитесь завершения обновления программы.");
                return true;
            }
            if invoke.message.webview_ref().label() != "main" {
                invoke.resolver.reject("Native commands are unavailable in this window");
                return true;
            }
            handler(invoke)
        })
        .build(context)
        .expect("error while building Unreal Manager")
        .run(|_,_| {});
}

pub fn run_app_update_helper(path:PathBuf)->Result<(),String> { app_update_apply::run(&path) }
pub fn repair_app_update(args:Vec<std::ffi::OsString>)->Result<bool,String> {app_update_apply::repair(args)}
pub fn report_app_repair_error(error:&str) {
    eprintln!("{error}");
    #[cfg(windows)] unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW,MB_OK,MB_ICONERROR};
        let text:Vec<u16>=format!("Не удалось завершить восстановление Unreal Manager. Файлы приложения могли быть восстановлены частично. Данные пользователя и сохранённые копии не удалялись. Не удаляйте папку восстановления; устраните указанную причину и повторите операцию.\n\n{error}").encode_utf16().chain(Some(0)).collect();
        let title:Vec<u16>="Unreal Manager — восстановление".encode_utf16().chain(Some(0)).collect();
        MessageBoxW(std::ptr::null_mut(),text.as_ptr(),title.as_ptr(),MB_OK|MB_ICONERROR);
    }
}
pub fn run_epic_registration_broker(path:PathBuf)->Result<(),String> { elevation::run(path) }
#[cfg(feature="engine-test-cli")]
pub fn epic_registration_uac_probe(path:PathBuf)->Result<(),String> { elevation::probe(path) }
pub fn app_update_startup()->Result<bool,String> { app_update_apply::recover_or_busy() }
pub fn report_app_update_error(error:&str) {
    eprintln!("{error}");
    #[cfg(windows)] unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW,MB_OK,MB_ICONERROR};
        let text:Vec<u16>=format!("Не удалось завершить обновление Unreal Manager. Автоматическое восстановление остановлено; существующие файлы сохранены.\n\n{error}").encode_utf16().chain(Some(0)).collect();
        let title:Vec<u16>="Unreal Manager — обновление".encode_utf16().chain(Some(0)).collect();
        MessageBoxW(std::ptr::null_mut(),text.as_ptr(),title.as_ptr(),MB_OK|MB_ICONERROR);
    }
}

/// Runs the same scanners used by the UI without starting WebView2.
pub fn write_scan_report(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = serde_json::json!({"engines": scan_engines(), "projects": scan_projects_local()});
    fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

#[cfg(debug_assertions)]
pub fn fab_info_report(path:PathBuf){
 use tauri::Manager;
 let mut context=tauri::generate_context!();context.config_mut().app.windows.clear();
 tauri::Builder::default().manage(epic::EpicState::default()).setup(move|app|{
  let handle=app.handle().clone();tauri::async_runtime::spawn(async move{
   let result=async {
    let cache=epic::fab_cached_library(handle.clone(),handle.state::<epic::EpicState>()).await?;
    let value=if let Some(cache)=cache {serde_json::to_value(cache).map_err(|e|e.to_string())?}else{serde_json::to_value(epic::fab_library(handle.clone(),handle.state::<epic::EpicState>(),tauri::ipc::Channel::new(|_|Ok(()))).await?).map_err(|e|e.to_string())?};
    let items=value["items"].as_array().ok_or("Empty library")?;
    let item=items.iter().find(|i|i["listingUrl"].as_str().is_some_and(|s|s.contains("/listings/"))).ok_or("No listing URL")?;
    let id=item["id"].as_str().ok_or("No ID")?.to_owned();
    let detail=fab_details::fab_details(handle.state::<epic::EpicState>(),id.clone(),true).await?;
    let image=fab_details::fab_detail_image(handle.state::<epic::EpicState>(),id.clone(),0).await;
    let catalog=epic::library_metadata(&handle.state::<epic::EpicState>(),&id).await;
    Ok::<_,String>(serde_json::json!({"items":items.len(),"id":id,"title":item["title"],"detail":detail,"catalogMetadata":catalog,"imageBytes":image.as_ref().ok().map(|s|s.len()),"imageError":image.err()}))
   }.await;
   let _=fs::write(path,serde_json::to_vec_pretty(&result).unwrap());handle.exit(0);
  });Ok(())
 }).run(context).expect("Fab info report");
}

pub fn engine_report(path: std::path::PathBuf) {
    let mut context=tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default().manage(epic::EpicState::default()).setup(move|app|{
        use tauri::Manager;
        let handle=app.handle().clone();
        tauri::async_runtime::spawn(async move {
            let result=epic::engine_assets(&handle,&handle.state::<epic::EpicState>()).await;
            let report=match result {Ok(assets)=>{ let metadata=epic::engine_metadata(&handle,&handle.state::<epic::EpicState>(),"UE_5.8").await; let manifest=epic::engine_manifest(&handle,&handle.state::<epic::EpicState>(),"UE_5.8").await; let m=match manifest {Ok((bytes,base,_))=>{let target=path.with_extension("manifest");let _=fs::write(&target,&bytes);serde_json::json!({"path":target,"base":base,"bytes":bytes.len()})},Err(e)=>serde_json::json!({"error":e})}; serde_json::json!({"assets":assets,"metadata":metadata,"manifest":m})},Err(e)=>serde_json::json!({"error":e})};
            let _=fs::write(path,serde_json::to_vec_pretty(&report).unwrap());
            handle.exit(0);
        });
        Ok(())
    }).run(context).expect("engine report");
}



pub fn engine_plan_report(manifest:PathBuf,target:PathBuf,report:PathBuf){let result=engine::local_report(&manifest,target);let _=fs::write(report,serde_json::to_vec_pretty(&result).unwrap());}




#[cfg(any(debug_assertions, feature = "engine-test-cli"))]
pub fn engine_test_install(raw_path:PathBuf,target:PathBuf,report:PathBuf){
 let mut context=tauri::generate_context!();context.config_mut().app.windows.clear();
 tauri::Builder::default().manage(epic::EpicState::default()).manage(engine::EngineState::default()).manage(vault::VaultState::default()).setup(move|app|{
  let app=app.handle().clone();tauri::async_runtime::spawn(async move{
   let result=engine::install_local(&app,raw_path,target,report.clone()).await;
   let _=fs::write(report.with_extension("result.json"),serde_json::to_vec_pretty(&result).unwrap());app.exit(0);
  });Ok(())
 }).run(context).expect("engine installation");
}



#[tauri::command]
fn open_author_profile()->Result<(),String>{webbrowser::open("https://github.com/MerryGrim").map_err(|e|e.to_string())}

pub fn repair_engine_registrations()->Result<usize,String>{storage::initialize()?;engine::install::repair_managed_registration()}

pub fn engine_resume_inspect(path:PathBuf,report:PathBuf){
 let result=engine::resume::inspect_legacy(&path.to_string_lossy());
 let body=match result{Ok(value)=>value,Err(error)=>serde_json::json!({"error":error})};let _=std::fs::write(report,serde_json::to_vec_pretty(&body).unwrap());
}
#[cfg(any(debug_assertions,feature="engine-test-cli"))]
pub fn engine_saved_task_inspect(base:PathBuf,id:String,report:PathBuf){
 let result=engine::resume::inspect_saved(&base,&id);
 let body=match result{Ok(value)=>value,Err(error)=>serde_json::json!({"error":error})};let _=std::fs::write(report,serde_json::to_vec_pretty(&body).unwrap());
}

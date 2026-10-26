use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicBool,AtomicU64, Ordering}}, time::{Duration, Instant}};
use serde::{Serialize, Deserialize};
use sha1::{Digest, Sha1};
use egs_api::api::types::download_manifest::{DownloadManifest, FileManifestList};


pub mod project_update;
pub mod transfers;
const CHUNK_LIMIT: usize = 64 * 1024 * 1024;
#[derive(Default)]
pub struct VaultState { plan: Mutex<Option<Plan>>, pub(crate) active: AtomicBool, pub(crate) cancel: AtomicBool, pub(crate) updating: AtomicBool, paused:AtomicBool, pause_generation:AtomicU64, pause_available:AtomicBool, channel:Mutex<Option<tauri::ipc::Channel<Progress>>>, progress_sink:Mutex<Option<std::sync::Arc<dyn Fn(Progress)+Send+Sync>>>, latest:Mutex<Option<Progress>>, rate:Mutex<Option<(Instant,u64,u64)>> }
#[derive(Clone,Serialize,Deserialize)]
struct Plan { info: Package, #[serde(default)] intent:Option<transfers::Intent>, #[serde(with="crate::engine::resume::snapshot_files")] files: BTreeMap<String, FileManifestList> }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Package { #[serde(default)] artifact_id: String, #[serde(default)] selected_engine: String, #[serde(default)] build_version: String, #[serde(default)] source: String, #[serde(default)] asset_id: String, pub(crate) id: String, title: String, kind: String, engines: Vec<String>, bytes: u64, download_bytes: u64, files: usize, path: String, hashes: BTreeMap<String, String> }
const PROJECT_DESTINATION_REQUIRED:&str="Это полный проект. Выберите новую папку для создания проекта.";
fn normalize_package_kind(info:&mut Package) {
    info.kind=crate::content_kind::from_paths(&info.kind,info.hashes.keys());
}
fn project_intent_needs_destination(info:&Package,intent:&transfers::Intent)->bool {
    info.kind=="Project"&&intent.mode=="install"&&Path::new(&intent.project).extension().is_some_and(|extension|extension.eq_ignore_ascii_case("uproject"))
}
fn normalize_plan_kind(plan:&mut Plan)->bool {
    let old=plan.info.kind.clone();normalize_package_kind(&mut plan.info);
    let reset=plan.intent.as_ref().is_some_and(|intent|plan.info.kind=="Project"&&intent.mode=="install"&&(old!="Project"||project_intent_needs_destination(&plan.info,intent)));
    if reset{plan.intent=None;}reset
}
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct Progress { pub(crate) phase: String, done: u64, total: u64, files_done: usize, files_total: usize, network_bytes: u64, bytes_per_second: u64, pub(crate) paused:bool, pub(crate) can_pause:bool }
fn io_error(error: std::io::Error) -> String { format!("Ошибка файловой системы: {error}") }
fn io_at(operation:&str,path:&Path,error:std::io::Error)->String{format!("Ошибка файловой системы ({operation}): {}: {error}",path.display())}
fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha1::digest(bytes)) }

pub(crate) fn relative(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 2000 { return Err("Некорректный путь в материале".into()); }
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
            || part.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit()) {
            return Err("Материал содержит недопустимый путь Windows".into());
        }
        out.push(part);
    }
    Ok(out)
}
pub(crate) fn no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)] { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return Err("Каталог содержит ссылку или точку перенаправления".into()); } }
                if meta.file_type().is_symlink() { return Err("Ссылки в каталоге не поддерживаются".into()); }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(io_error(e)),
        }
    }
    Ok(())
}
pub(crate) fn hash_file(path: &Path) -> Result<String, String> {
    hash_file_progress(path, |_| {})
}
pub(crate) fn hash_file_progress(path:&Path,mut progress:impl FnMut(u64))->Result<String,String>{
    no_links(path)?;
    let mut file = File::open(path).map_err(|e|io_at("Чтение файла",path,e))?;
    let mut hash = Sha1::new(); let mut buffer = [0u8; 65536];
    loop { let n = file.read(&mut buffer).map_err(|e|io_at("Чтение файла",path,e))?; if n == 0 { break; } hash.update(&buffer[..n]); progress(n as u64); }
    Ok(format!("{:x}", hash.finalize()))
}
fn local_progress(phase:&str,done:u64,total:u64,files_done:usize,files_total:usize)->Progress{
    Progress{phase:phase.into(),done,total,files_done,files_total,network_bytes:0,bytes_per_second:0,paused:false,can_pause:false}
}
struct FileProgress<'a>{phase:&'static str,done:u64,total:u64,files_done:usize,files_total:usize,last:Instant,interval:Duration,notify:&'a mut dyn FnMut(Progress)}
impl<'a> FileProgress<'a>{
    fn new(phase:&'static str,total:u64,files_total:usize,notify:&'a mut dyn FnMut(Progress))->Self{
        Self::with_interval(phase,total,files_total,Duration::from_millis(250),notify)
    }
    fn with_interval(phase:&'static str,total:u64,files_total:usize,interval:Duration,notify:&'a mut dyn FnMut(Progress))->Self{let mut p=Self{phase,done:0,total,files_done:0,files_total,last:Instant::now(),interval,notify};p.send(false);p}
    fn send(&mut self,complete:bool){
        // A full buffer read is not a verified file. Reserve the final byte until checks pass.
        let done=if complete{self.done.min(self.total)}else{self.done.min(self.total.saturating_sub(1))};
        (self.notify)(local_progress(self.phase,done,self.total,self.files_done,self.files_total));self.last=Instant::now();
    }
    fn advance(&mut self,bytes:u64){self.done=self.done.saturating_add(bytes);if self.last.elapsed()>=self.interval{self.send(false);}}
    fn file_done(&mut self){self.files_done+=1;if self.files_done==self.files_total||self.last.elapsed()>=self.interval{self.send(self.files_done==self.files_total);}}
}
async fn hash_download_file(state:&VaultState,path:&Path,mut progress:Progress)->Result<(String,u64),String>{
    no_links(path)?;let mut file=File::open(path).map_err(io_error)?;let mut hash=Sha1::new();let mut buffer=vec![0u8;65536];let mut last=Instant::now();let mut since_yield=0;let initial_done=progress.done;
    transfers::emit(state,progress.clone());
    loop{
        transfers::checkpoint(state).await?;
        let n=file.read(&mut buffer).map_err(io_error)?;if n==0{break;}hash.update(&buffer[..n]);progress.done=progress.done.saturating_add(n as u64);
        if last.elapsed()>=Duration::from_millis(250){let mut pending=progress.clone();pending.done=pending.done.min(pending.total.saturating_sub(1));transfers::emit(state,pending);last=Instant::now();}
        since_yield+=n;if since_yield>=1024*1024{tokio::task::yield_now().await;since_yield=0;}
    }
    Ok((format!("{:x}",hash.finalize()),progress.done.saturating_sub(initial_done)))
}
fn valid_hash(hash: &str) -> bool { hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_hexdigit()) }
const EPIC_CACHE_SOURCE:&str="Epic Games Launcher";
fn same_cached_files(left:&Package,right:&Package)->bool{left.asset_id==right.asset_id&&crate::content_kind::from_paths(&left.kind,left.hashes.keys())==crate::content_kind::from_paths(&right.kind,right.hashes.keys())&&left.hashes==right.hashes}
fn normalized_engine(value:&str)->String{let value=value.trim();value.get(..3).filter(|p|p.eq_ignore_ascii_case("UE_")).map(|_|&value[3..]).unwrap_or(value).to_ascii_lowercase()}
fn covers_requested_engines(cached:&Package,fresh:&Package)->bool{
    let declared:BTreeSet<_>=cached.engines.iter().map(|e|normalized_engine(e)).filter(|e|!e.is_empty()).collect();
    !fresh.engines.is_empty()&&fresh.engines.iter().all(|e|{let e=normalized_engine(e);!e.is_empty()&&declared.contains(&e)})
}
// Content identity can share physical files, but never replaces the requested
// artifact, publisher compatibility, selected engine, title or build metadata.
fn select_prepared_cache(mut plan:Plan,vault:&Path,external:Vec<Package>)->Plan{
    if vault.join(&plan.info.id).exists(){return plan;}
    if let Some(cached)=external.into_iter().find(|p|p.source==EPIC_CACHE_SOURCE&&same_cached_files(p,&plan.info)&&covers_requested_engines(p,&plan.info)&&Path::new(&p.path).is_dir()){
        plan.info.id=cached.id;plan.info.path=cached.path;plan.info.source=cached.source;
    }
    plan
}
fn check_external_alias(info:&Package,index:&[Package])->Result<(),String>{
    let found=index.iter().any(|p|p.source==EPIC_CACHE_SOURCE&&p.id==info.id&&p.path==info.path&&same_cached_files(p,info)&&covers_requested_engines(p,info));
    if info.source!=EPIC_CACHE_SOURCE||!valid_hash(&info.id)||!found||!Path::new(&info.path).is_dir(){return Err("Папка Epic Vault недоступна. Выполните синхронизацию".into());}
    no_links(Path::new(&info.path))
}
fn make_plan(item: super::epic::LibraryItem, artifact: &str, manifest: DownloadManifest) -> Result<Plan, String> {
    if manifest.b_is_file_data { return Err("Этот старый формат загрузки пока не поддерживается".into()); }
    let mut seen = BTreeSet::new(); let mut hashes = BTreeMap::new();
    for file in &manifest.file_manifest_list {
        let path = relative(&file.filename)?;
        if path.to_string_lossy().eq_ignore_ascii_case("package.um.json") || !seen.insert(path.to_string_lossy().to_lowercase()) || !valid_hash(&file.file_hash) { return Err("Некорректное описание файлов Fab".into()); }
        hashes.insert(file.filename.clone(), file.file_hash.to_ascii_lowercase());
    }
    let files: BTreeMap<_, _> = manifest.files().into_iter().collect();
    if files.is_empty() || files.len() != manifest.file_manifest_list.len() { return Err("Fab не вернул полный список файлов".into()); }
    for original in &manifest.file_manifest_list {
        let file = &files[&original.filename];
        if file.file_chunk_parts.len() != original.file_chunk_parts.len() { return Err("Fab не вернул адреса всех блоков".into()); }
        for part in &file.file_chunk_parts {
            let url = part.link.as_ref().ok_or("Отсутствует адрес блока")?;
            if url.scheme() != "https" || part.offset.checked_add(part.size).is_none_or(|n| n > CHUNK_LIMIT as u128) {
                return Err("Недопустимый блок загрузки".into());
            }
        }
    }
    let fingerprint = serde_json::to_string(&hashes).map_err(|_| "Не удалось сравнить описание файлов")?;
    let id = digest(format!("{}\n{artifact}\n{}\n{fingerprint}", item.id, manifest.build_version_string).as_bytes());
    let engines = item.versions.iter().find(|v| v.artifact_id == artifact).map(|v| v.engines.clone()).unwrap_or_default();
    let kind=crate::content_kind::from_paths(item.declared_kind(),hashes.keys());
    Ok(Plan { intent:None, info: Package { artifact_id: artifact.into(), selected_engine: String::new(), build_version: manifest.build_version_string.clone(), source: String::new(), asset_id: item.id.clone(), path: crate::storage::vault().as_path().join(&id).to_string_lossy().into_owned(), id, title: item.title, kind, engines,
        bytes: manifest.total_size().try_into().map_err(|_| "Материал слишком велик")?, download_bytes: manifest.total_download_size().try_into().map_err(|_| "Материал слишком велик")?, files: files.len(), hashes }, files })
}

#[tauri::command]
pub async fn vault_prepare(app: tauri::AppHandle, epic: tauri::State<'_, super::epic::EpicState>, state: tauri::State<'_, VaultState>, id: String, artifact: String, engine_version: String) -> Result<Package, String> {
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true,Ordering::SeqCst) { return Err("Дождитесь текущей загрузки".into()); }
    let _busy=Busy(&state.active);
    let plan=prepare_locked(&app,&epic,&id,&artifact,&engine_version,None).await?;
    let info=plan.info.clone();
    *state.plan.lock().map_err(|_| "Ошибка состояния загрузки")?=Some(plan);
    Ok(info)
}
async fn prepare_locked(app:&tauri::AppHandle,epic:&crate::epic::EpicState,id:&str,artifact:&str,engine_version:&str,account:Option<&str>)->Result<Plan,String>{
    let (item, manifest) = super::epic::download_manifest_bound(app,epic,id,artifact,account).await?;
    if !item.versions.iter().any(|v| v.artifact_id==artifact && v.engines.iter().any(|e|e.trim_start_matches("UE_")==engine_version) && (v.platforms.is_empty() || v.platforms.iter().any(|p|p.to_lowercase().contains("win")))) {
        return Err("Выбранный пакет не поддерживает указанную версию Unreal Engine для Windows".into());
    }
    let mut plan = make_plan(item, &artifact, manifest)?;
    plan.info.selected_engine=engine_version.into();
    super::epic::remember_library_kind(epic,&plan.info.asset_id,&plan.info.kind).await;
    Ok(select_prepared_cache(plan,&crate::storage::vault(),epic_index()?.items))
}

pub(crate) fn decode_chunk(buffer: &[u8]) -> Result<Vec<u8>, String> {
    let err = || "Повреждён блок загрузки".to_string();
    let u32at = |offset: usize| -> Result<u32, String> { Ok(u32::from_le_bytes(buffer.get(offset..offset+4).ok_or_else(err)?.try_into().map_err(|_| err())?)) };
    if buffer.len() < 41 || u32at(0)? != 2986228386 { return Err(err()); }
    let version = u32at(4)?; let header = u32at(8)? as usize; let compressed = u32at(12)? as usize;
    let minimum = match version { 1 => 41, 2 => 62, 3 => 66, _ => return Err(err()) };
    if header < minimum || header > buffer.len() || compressed != buffer.len()-header || buffer[40] > 1 { return Err(err()); }
    let data = if buffer[40] == 1 {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&buffer[header..]).take(CHUNK_LIMIT as u64 + 1).read_to_end(&mut out).map_err(|_| err())?; out
    } else { buffer[header..].to_vec() };
    if data.len() > CHUNK_LIMIT || (version >= 3 && u32at(62)? as usize != data.len()) { return Err(err()); }
    if version >= 2 && buffer[61] & 2 != 0 && Sha1::digest(&data).as_slice() != &buffer[41..61] { return Err(err()); }
    Ok(data)
}
pub(crate) struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> { fn drop(&mut self) { self.0.store(false, Ordering::SeqCst);crate::vault_queue::wake(); } }
#[tauri::command]
pub fn vault_cancel(state: tauri::State<'_, VaultState>) { state.cancel.store(true, Ordering::SeqCst); }

#[tauri::command]
pub async fn vault_download(app:tauri::AppHandle,state: tauri::State<'_, VaultState>, id: String, on_progress: tauri::ipc::Channel<Progress>,intent:Option<transfers::Intent>) -> Result<Package, String> {
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true, Ordering::SeqCst) { return Err("Уже выполняется загрузка".into()); }
    let _busy = Busy(&state.active); state.cancel.store(false, Ordering::SeqCst);
    if let Some(intent)=intent {if !matches!(intent.mode.as_str(),"vault"|"install")||intent.project.len()>2000||intent.project_name.len()>100||intent.engine_version.len()>100{return Err("Некорректное действие после загрузки".into());}let mut plan=state.plan.lock().map_err(|_|"Ошибка состояния")?;let prepared=plan.as_mut().filter(|p|p.info.id==id).ok_or("Подготовьте загрузку заново")?;if project_intent_needs_destination(&prepared.info,&intent){return Err(PROJECT_DESTINATION_REQUIRED.into());}prepared.intent=Some(intent);}
    let result=download_locked(&state,&id,Some(on_progress)).await;
    if let Ok(package)=&result{crate::vault_queue::manual_download_completed(&app,package,&package.asset_id,&package.path);}
    let install_reserved=state.plan.lock().map(|p|p.as_ref().and_then(|p|p.intent.as_ref()).is_some_and(|i|i.mode=="install")).unwrap_or(true);
    if result.is_ok()&&!install_reserved{discard_plan(&state);}
    result
}
async fn download_locked(state:&VaultState,id:&str,on_progress:Option<tauri::ipc::Channel<Progress>>)->Result<Package,String>{
    download_locked_from(state,id,on_progress,&crate::storage::vault(),&||Ok(epic_index()?.items)).await
}
async fn download_locked_from(state:&VaultState,id:&str,on_progress:Option<tauri::ipc::Channel<Progress>>,vault:&Path,external_index:&(impl Fn()->Result<Vec<Package>,String>+Sync))->Result<Package,String>{
    let _scope=transfers::Scope(state);state.paused.store(false,Ordering::SeqCst);*state.channel.lock().map_err(|_|"Ошибка состояния")?=on_progress;*state.rate.lock().map_err(|_|"Ошибка состояния")?=None;
    let plan = state.plan.lock().map_err(|_| "Ошибка состояния")?.as_ref().filter(|p| p.info.id == id).cloned().ok_or("Подготовьте загрузку заново")?;
    let destination = Path::new(&plan.info.path);
    no_links(destination)?;
    let external=!plan.info.source.is_empty();
    // An unavailable external cache must never fall through to the download
    // branch: its physical directory is not owned by Unreal Manager.
    if external{check_external_alias(&plan.info,&external_index()?)?;}
    else if !valid_hash(id)||destination!=vault.join(id){return Err("Хранилище изменено. Верните исходную папку Vault для продолжения этой загрузки.".into());}
    if external || destination.exists() {
        if !external{let cached=package_at(vault,id)?;if !cached.source.is_empty()||!same_cached_files(&cached,&plan.info){return Err("Некорректное описание Vault".into());}}
        state.pause_available.store(true,Ordering::SeqCst);
        let mut done=0;let mut files=0;
        for (name,hash) in &plan.info.hashes {
            let (actual,bytes)=hash_download_file(state,&destination.join(relative(name)?),local_progress("Проверяем файлы Vault",done,plan.info.bytes,files,plan.info.files)).await?;
            if actual != *hash { return Err("Файлы Vault повреждены. Повторное скачивание не начато".into()); }
            done+=bytes;files+=1;transfers::emit(state,local_progress("Проверяем файлы Vault",done,plan.info.bytes,files,plan.info.files));
        }
        transfers::checkpoint(state).await?;state.pause_available.store(false,Ordering::SeqCst);
        if external{check_external_alias(&plan.info,&external_index()?)?;}
        else{
            no_links(destination)?;
            let record=serde_json::to_vec_pretty(&plan.info).map_err(|_|"Не удалось сохранить описание материала")?;
            crate::engine::install::atomic(&destination.join("package.um.json"),&record)?;
            let stage=vault.join(".partial").join(id);if stage.exists(){transfers::cleanup(&stage,id)?;}else{transfers::remove(id)?;}
        }
        return Ok(plan.info);
    }
    let staging = vault.join(".partial").join(&id);
    no_links(&staging)?; fs::create_dir_all(staging.join("files")).map_err(io_error)?;
    let saved=plan.clone();tauri::async_runtime::spawn_blocking(move||transfers::save(&saved)).await.map_err(|e|e.to_string())??;
    state.pause_available.store(true,Ordering::SeqCst);
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "Не удалось создать загрузчик")?;
    let mut last_event = Instant::now(); let mut done = 0; let mut network = 0; let mut completed = 0;
    let mut cache: Option<(String, Vec<u8>)> = None;
    let emit = |phase: &str, done, completed, network| { transfers::emit(state,Progress { phase: phase.into(), done, total: plan.info.bytes, files_done: completed, files_total: plan.files.len(), network_bytes: network, bytes_per_second: 0,paused:false,can_pause:false }); };
    emit("Загружаем и проверяем файлы", done, completed, network);
    for (name, file) in &plan.files {
        transfers::checkpoint(state).await?;
        let target = staging.join("files").join(relative(name)?); no_links(&target)?;
        if target.exists() && hash_download_file(state,&target,Progress{network_bytes:network,..local_progress("Проверяем ранее загруженные файлы",done,plan.info.bytes,completed,plan.files.len())}).await?.0 == file.file_hash.to_ascii_lowercase() { done += file.size() as u64; completed += 1; emit("Проверяем ранее загруженные файлы", done, completed, network); continue; }
        if target.exists() { return Err("Ранее загруженный файл повреждён. Сохраните его отдельно и повторите загрузку".into()); }
        fs::create_dir_all(target.parent().ok_or("Некорректный путь")?).map_err(io_error)?;
        let temporary = staging.join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        let _temporary=transfers::Temporary(temporary.clone());
        let mut output = OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(io_error)?;
        let mut hash = Sha1::new();
        let mut prepared_in_file=0u64;
        for part in &file.file_chunk_parts {
            transfers::checkpoint(state).await?;
            if cache.as_ref().is_none_or(|(guid,_)| guid != &part.guid) {
                let data = transfers::chunk(&client,part.link.as_ref().ok_or("Нет адреса блока")?,&part.guid,&staging.join("chunks"),state,&mut network).await?;
                cache = Some((part.guid.clone(),data));
            }
            let data = &cache.as_ref().unwrap().1;
            let slice = data.get(part.offset as usize..(part.offset + part.size) as usize).ok_or("Неверный размер блока")?;
            output.write_all(slice).map_err(io_error)?; hash.update(slice);
            prepared_in_file+=slice.len() as u64;
            if last_event.elapsed() >= Duration::from_millis(250) { emit("Загружаем и проверяем файлы", done+prepared_in_file, completed, network); last_event = Instant::now(); }
        }
        output.sync_all().map_err(io_error)?; drop(output);
        if format!("{:x}", hash.finalize()) != file.file_hash.to_ascii_lowercase() { return Err("Контрольная сумма файла не совпала. Материал не установлен".into()); }
        fs::rename(&temporary, &target).map_err(io_error)?;
        done += file.size() as u64; completed += 1;
        if last_event.elapsed() >= Duration::from_millis(250) { emit("Загружаем и проверяем файлы", done, completed, network); last_event = Instant::now(); }
    }
    transfers::checkpoint(state).await?;state.pause_available.store(false,Ordering::SeqCst);emit("Завершаем загрузку",done,completed,network);
    no_links(destination)?;
    let record = serde_json::to_vec_pretty(&plan.info).map_err(|_| "Не удалось сохранить описание материала")?;
    no_links(&staging.join("files/package.um.json"))?;crate::storage::atomic(&staging.join("files/package.um.json"),&record)?;
    fs::rename(staging.join("files"), destination).map_err(io_error)?;
    transfers::cleanup(&staging,id)?;
    emit("Готово — файлы проверены", done, completed, network);
    Ok(plan.info)
}

fn package(id: &str) -> Result<Package, String> {
    if !valid_hash(id) { return Err("Некорректный материал Vault".into()); }
    if let Some(mut info)=epic_index()?.items.into_iter().find(|p|p.id==id) {
        no_links(Path::new(&info.path))?;
        if !Path::new(&info.path).is_dir() {return Err("Папка Epic Vault недоступна. Выполните синхронизацию".into());}
        for (name,hash) in &info.hashes {relative(name)?;if !valid_hash(hash){return Err("Повреждён индекс Epic Vault".into());}}
        normalize_package_kind(&mut info);
        return Ok(info);
    }
    package_at(&crate::storage::vault(),id)
}
fn package_at(vault:&Path,id:&str)->Result<Package,String>{
    if !valid_hash(id){return Err("Некорректный материал Vault".into());}
    let root = vault.join(id); no_links(&root)?;
    let record = root.join("package.um.json");
    let mut bytes = Vec::new(); File::open(record).map_err(io_error)?.take(32*1024*1024).read_to_end(&mut bytes).map_err(io_error)?;
    let mut result: Package = serde_json::from_slice(&bytes).map_err(|_| "Повреждено описание Vault")?;
    if result.id != id { return Err("Некорректное описание Vault".into()); }
    result.path = root.to_string_lossy().into_owned();
    for (name, hash) in &result.hashes { relative(name)?; if !valid_hash(hash) { return Err("Повреждено описание Vault".into()); } }
    normalize_package_kind(&mut result);
    Ok(result)
}
#[tauri::command]
pub fn vault_list() -> Result<Vec<Package>, String> {
    no_links(crate::storage::vault().as_path())?;
    
    let mut results = Vec::new();
    for entry in if crate::storage::vault().exists() {Some(fs::read_dir(crate::storage::vault()).map_err(io_error)?)}else{None}.into_iter().flatten() { let entry=entry.map_err(io_error)?; let id=entry.file_name().to_string_lossy().into_owned(); if valid_hash(&id) { results.push(package(&id)?); } }
    results.extend(epic_index()?.items.into_iter().filter(|p|Path::new(&p.path).is_dir()));
    results.sort_by_key(|p| p.title.to_lowercase()); Ok(results)
}

// One bounded, metadata-only pass for library reconciliation. Never walk asset
// contents, request network data or rewrite an existing package/task record.
pub(crate) fn known_content_kinds()->BTreeMap<String,String> {
    fn add(known:&mut BTreeMap<String,String>,mut info:Package) {
        if info.asset_id.is_empty()||info.asset_id.len()>256||info.hashes.is_empty()||info.files!=info.hashes.len()
            ||info.hashes.iter().any(|(name,hash)|relative(name).is_err()||!valid_hash(hash)){return;}
        normalize_package_kind(&mut info);
        if !matches!(info.kind.as_str(),"Project"|"Plugin"){return;}
        known.entry(info.asset_id).and_modify(|kind|{if info.kind=="Project"{*kind=info.kind.clone();}}).or_insert(info.kind);
    }
    let mut known=BTreeMap::new();let vault=crate::storage::vault();let mut budget=64u64*1024*1024;
    if no_links(&vault).is_ok(){
        for entry in fs::read_dir(&vault).into_iter().flatten().flatten().take(4096){
            let id=entry.file_name().to_string_lossy().into_owned();if !valid_hash(&id){continue;}
            let record=entry.path().join("package.um.json");if no_links(&record).is_err(){continue;}let Ok(size)=fs::metadata(&record).map(|m|m.len())else{continue};
            if size>32*1024*1024||size>budget{continue;}budget-=size;
            if let Ok(info)=package_at(&vault,&id){add(&mut known,info);}
        }
    }
    let index=crate::storage::settings().join("epic-vault.json");
    if no_links(&index).is_ok()&&fs::metadata(&index).is_ok_and(|m|m.len()<=32*1024*1024){
        if let Ok(index)=epic_index(){for info in index.items.into_iter().take(4096){if Path::new(&info.path).is_dir(){add(&mut known,info);}}}
    }
    for info in transfers::known_packages().into_values(){add(&mut known,info);}
    known
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct InstallPreview { target: String, files: usize, conflicts: Vec<String>, existing: usize, compatible: bool, project_file: Option<String> }

fn checked_vault_folder(root:&Path, target:&Path)->Result<PathBuf,String>{
    no_links(root)?;no_links(target)?;
    let root=root.canonicalize().map_err(io_error)?;
    let target=target.canonicalize().map_err(io_error)?;
    if target.parent()!=Some(root.as_path()) || !target.is_dir(){return Err("Удаление разрешено только для отдельной папки материала внутри Vault".into());}
    let mut pending=vec![target.clone()];
    while let Some(folder)=pending.pop(){
        for entry in fs::read_dir(folder).map_err(io_error)? {
            let path=entry.map_err(io_error)?.path();no_links(&path)?;
            if fs::symlink_metadata(&path).map_err(io_error)?.is_dir(){pending.push(path);}
        }
    }
    Ok(target)
}
fn delete_targets(ids:&[String])->Result<Vec<PathBuf>,String>{
    if ids.is_empty(){return Err("Материал не скачан".into());}
    let index=epic_index()?;
    let mut targets=BTreeSet::new();
    for id in ids {
        let info=package(id)?;
        let target=if index.items.iter().any(|p|p.id==*id){
            let data=Path::new(&info.path);
            if data.file_name().and_then(|n|n.to_str())!=Some("data"){return Err("Некорректная папка Epic Vault".into());}
            let folder=data.parent().ok_or("Некорректная папка Epic Vault")?;
            no_links(&folder.join("manifest"))?;
            if !folder.join("manifest").is_file(){return Err("Манифест Epic Vault не найден".into());}
            checked_vault_folder(Path::new(&index.directory),folder)?
        }else{checked_vault_folder(crate::storage::vault().as_path(),&crate::storage::vault().as_path().join(id))?};
        targets.insert(target);
    }
    Ok(targets.into_iter().collect())
}
#[tauri::command]
pub async fn vault_delete_preview(ids:Vec<String>)->Result<Vec<String>,String>{
    tauri::async_runtime::spawn_blocking(move||delete_targets(&ids).map(|paths|paths.iter().map(|p|p.to_string_lossy().into_owned()).collect())).await.map_err(|_|"Проверка удаления прервана")?
}
#[tauri::command]
pub async fn vault_delete(state:tauri::State<'_,VaultState>,ids:Vec<String>,expected_paths:Vec<String>,mode:Option<super::project_actions::DeleteMode>)->Result<(),String>{
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции Vault".into());}let _busy=Busy(&state.active);
    let result=tauri::async_runtime::spawn_blocking(move||{
        let targets=delete_targets(&ids)?;
        if targets.iter().map(|p|p.to_string_lossy().into_owned()).collect::<Vec<_>>()!=expected_paths{return Err("Папки изменились. Откройте подтверждение удаления заново".into());}
        for target in targets {no_links(&target)?;match mode.unwrap_or(super::project_actions::DeleteMode::Permanent){super::project_actions::DeleteMode::Recycle=>super::project_actions::recycle_folder(&target)?,super::project_actions::DeleteMode::Permanent=>fs::remove_dir_all(target).map_err(io_error)?,}}
        Ok(())
    }).await.map_err(|_|"Удаление прервано")?;
    *state.plan.lock().map_err(|_|"Ошибка состояния Vault")?=None;
    result
}

#[tauri::command]
pub async fn vault_reveal(id:String)->Result<(),String>{
    tauri::async_runtime::spawn_blocking(move||{
        let info=package(&id)?;
        let path=Path::new(&info.path);no_links(path)?;
        if !path.is_dir(){return Err("Папка материала не найдена. Проверьте хранилище и обновите список".into());}
        let canonical=path.canonicalize().map_err(io_error)?;
        let value=canonical.to_string_lossy();
        let location=if let Some(unc)=value.strip_prefix(r"\\?\UNC\") {format!(r"\\{}",unc)}else{value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()};
        std::process::Command::new("explorer.exe").arg(location).spawn().map_err(|e|format!("Не удалось открыть папку: {e}"))?;
        Ok(())
    }).await.map_err(|e|e.to_string())?
}
fn validate_project_name(name:&str)->Result<(),String>{
    if name.is_empty() || name.len()>64 || !name.as_bytes()[0].is_ascii_alphabetic() || !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_') {return Err("Имя проекта: латинские буквы, цифры и подчёркивание; начните с буквы (до 64 символов)".into());}
    relative(&format!("{name}.uproject"))?;Ok(())
}
fn plugin_engine_root(info:&Package,selected:&Path,engines:&[super::EngineInstall])->Result<PathBuf,String>{
    no_links(selected)?;
    let canonical=selected.canonicalize().map_err(io_error)?;
    let engine=engines.iter().find(|e|Path::new(&e.path).canonicalize().is_ok_and(|p|p==canonical)).ok_or("Выберите установленный Unreal Engine из списка")?;
    if !info.engines.iter().any(|v|v.trim_start_matches("UE_")==engine.version){return Err("Эта версия плагина не поддерживает выбранный движок. Скачайте подходящую версию из библиотеки".into());}
    let root=canonical.join("Engine");no_links(&root)?;Ok(root)
}
fn plugin_destination(engine:bool,name:&std::ffi::OsStr,suffix:&Path)->PathBuf{
    Path::new(if engine{"Plugins/Marketplace"}else{"Plugins"}).join(name).join(suffix)
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct InstalledPlugin {name:String,title:String,engine:String,engine_path:String,path:String,vault_ids:Vec<String>}
fn engine_plugins()->Result<Vec<InstalledPlugin>,String>{
    let packages=vault_list()?;let mut result=Vec::new();
    for engine in super::scan_engines(){
        let root=Path::new(&engine.path).join("Engine/Plugins/Marketplace");
        if !root.is_dir(){continue;}no_links(&root)?;
        for folder in fs::read_dir(&root).map_err(io_error)?{
            let folder=folder.map_err(io_error)?.path();no_links(&folder)?;if !folder.is_dir(){continue;}
            for entry in fs::read_dir(&folder).map_err(io_error)?{
                let file=entry.map_err(io_error)?.path();if !file.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("uplugin")){continue;}
                no_links(&file)?;
                let mut bytes=Vec::new();File::open(&file).map_err(io_error)?.take(1024*1024).read_to_end(&mut bytes).map_err(io_error)?;
                let descriptor:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|"Не удалось прочитать описание установленного плагина")?;
                let name=file.file_stem().ok_or("Некорректное имя плагина")?.to_string_lossy().into_owned();
                let ids=packages.iter().filter(|p|p.kind=="Plugin"&&p.hashes.keys().any(|f|Path::new(f).file_name().is_some_and(|n|n.to_string_lossy().eq_ignore_ascii_case(&format!("{name}.uplugin"))))).map(|p|p.id.clone()).collect();
                result.push(InstalledPlugin{name:name.clone(),title:descriptor["FriendlyName"].as_str().unwrap_or(&name).into(),engine:engine.version.clone(),engine_path:engine.path.clone(),path:folder.to_string_lossy().into_owned(),vault_ids:ids});
            }
        }
    }
    Ok(result)
}
#[tauri::command]
pub async fn plugins_installed()->Result<Vec<InstalledPlugin>,String>{
    tauri::async_runtime::spawn_blocking(engine_plugins).await.map_err(|_|"Поиск плагинов прерван")?
}
fn checked_engine_plugin(root:&Path,path:&Path,name:&str)->Result<PathBuf,String>{
    relative(&format!("{name}.uplugin"))?;
    if name.contains(['/', '\\']){return Err("Некорректное имя плагина".into());}
    let target=checked_vault_folder(root,path)?;
    let descriptor=target.join(format!("{name}.uplugin"));
    if !descriptor.is_file(){return Err("Плагин больше не найден на движке".into());}
    for entry in walkdir::WalkDir::new(&target).follow_links(false){
        let path=entry.map_err(|_|"Не удалось проверить папку плагина")?.into_path();
        if path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("uplugin"))&&path!=descriptor{return Err("В папке есть другой плагин. Удаление заблокировано".into());}
    }
    Ok(target)
}
#[tauri::command]
pub async fn plugin_remove_engine(state:tauri::State<'_,VaultState>,name:String,engine_path:String,expected_path:String)->Result<(),String>{
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции Vault".into());}let _busy=Busy(&state.active);
    tauri::async_runtime::spawn_blocking(move||{
        let installs=engine_plugins()?;
        let found=installs.iter().find(|p|p.name==name&&p.engine_path==engine_path&&p.path==expected_path).ok_or("Установка изменилась. Обновите список плагинов")?;
        let target=checked_engine_plugin(&Path::new(&engine_path).join("Engine/Plugins/Marketplace"),Path::new(&found.path),&name)?;
        fs::remove_dir_all(target).map_err(io_error)
    }).await.map_err(|_|"Удаление плагина прервано")?
}
fn install_files(info: &Package, project: &str, project_name: Option<&str>) -> Result<(PathBuf, BTreeMap<String, PathBuf>, bool), String> {
    install_files_mode(info, project, project_name, false)
}
fn install_files_mode(info: &Package, project: &str, project_name: Option<&str>, updating: bool) -> Result<(PathBuf, BTreeMap<String, PathBuf>, bool), String> {
    let selected = PathBuf::from(project);
    if !selected.is_absolute() { return Err("Укажите полный путь назначения".into()); }
    no_links(&selected)?;
    if info.kind == "Project" {
        if selected.exists() && !updating { return Err("Для нового проекта выберите ещё не существующий каталог".into()); }
        if !info.hashes.keys().any(|n| n.to_lowercase().ends_with(".uproject")) { return Err("В материале нет файла проекта Unreal Engine".into()); }
        let mut entries:BTreeMap<String,PathBuf> = info.hashes.keys().map(|n| Ok((n.clone(), relative(n)?))).collect::<Result<_, String>>()?;
        if let Some(name)=project_name {
            validate_project_name(name)?;
            let descriptors:Vec<_>=entries.keys().filter(|n|n.to_lowercase().ends_with(".uproject")).cloned().collect();
            if descriptors.len()!=1 {return Err("Для задания имени в пакете должен быть один файл .uproject".into());}
            let source=&descriptors[0];let old=relative(source)?;
            let old_name=old.file_stem().and_then(|s|s.to_str()).ok_or("Некорректное имя проекта")?;
            if old_name!=name && entries.keys().any(|p|p.split('/').any(|part|part.eq_ignore_ascii_case("Source")) || p.to_lowercase().ends_with(".target.cs")) {
                return Err(format!("Этот проект содержит C++-код. Пока используйте исходное имя файла: {old_name}. Переименование C++-модулей ещё не поддерживается"));
            }
            entries.insert(source.clone(),old.with_file_name(format!("{name}.uproject")));
            let icon=old.with_extension("png").to_string_lossy().replace('\\',"/");
            if entries.contains_key(&icon){entries.insert(icon,old.with_file_name(format!("{name}.png")));}
            let mut seen=BTreeSet::new();
            if entries.values().any(|p|!seen.insert(p.to_string_lossy().to_lowercase())){return Err("Новое имя конфликтует с файлом пакета".into());}
        }
        return Ok((selected, entries, true));
    }
    let engine_target=info.kind=="Plugin"&&selected.is_dir();
    let (root,compatible)=if engine_target {(plugin_engine_root(info,&selected,&super::scan_engines())?,true)}else{
    if selected.extension().and_then(|x| x.to_str()) != Some("uproject") { return Err("Выберите файл .uproject".into()); }
    let descriptor: serde_json::Value = serde_json::from_slice(&fs::read(&selected).map_err(io_error)?).map_err(|_| "Не удалось прочитать проект")?;
    let association = descriptor["EngineAssociation"].as_str().unwrap_or("");
    let compatible = info.engines.iter().any(|v| v.trim_start_matches("UE_") == association);
    let root = selected.parent().ok_or("Не найден каталог проекта")?.to_path_buf();
    (root,compatible)};
    let mut entries = BTreeMap::new();
    if info.kind == "Plugin" {
        let plugins: Vec<_> = info.hashes.keys().filter(|n| n.to_lowercase().ends_with(".uplugin")).collect();
        if plugins.len() != 1 { return Err("Автоматическая установка поддерживает пакет с одним плагином".into()); }
        let descriptor = relative(plugins[0])?;
        let prefix = descriptor.parent().ok_or("Некорректный путь плагина")?;
        let plugin_name = descriptor.file_stem().ok_or("У плагина нет имени")?;
        for name in info.hashes.keys() {
            let source = relative(name)?;
            if let Ok(suffix) = source.strip_prefix(prefix) { entries.insert(name.clone(), plugin_destination(engine_target,plugin_name,suffix)); }
        }
    } else {
        for name in info.hashes.keys() {
            let source = relative(name)?;
            let components: Vec<_> = source.components().collect();
            if let Some(index) = components.iter().position(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("Content")) {
                let mapped: PathBuf = components[index..].iter().collect(); entries.insert(name.clone(), mapped);
            }
        }
    }
    if entries.is_empty() { return Err("Нет файлов Content или плагина для добавления в проект".into()); }
    let mut seen=BTreeSet::new();
    for path in entries.values() { if !seen.insert(path.to_string_lossy().to_lowercase()) { return Err("В пакете есть конфликтующие пути".into()); } }
    Ok((root, entries, compatible))
}
fn preview(info: &Package, project: &str, project_name: Option<&str>) -> Result<InstallPreview, String> {
    preview_progress(info,project,project_name,&mut |_|{})
}
fn preview_progress(info:&Package,project:&str,project_name:Option<&str>,notify:&mut dyn FnMut(Progress))->Result<InstallPreview,String>{
    let (root, entries, compatible) = install_files(info, project, project_name)?;
    notify(local_progress("Проверяем файлы проекта",0,0,0,entries.len()));
    let mut bytes=0u64;
    for path in entries.values(){let target=root.join(path);no_links(&target)?;if target.is_file(){bytes=bytes.saturating_add(fs::metadata(target).map_err(io_error)?.len());}}
    let mut progress=FileProgress::new("Проверяем файлы проекта",bytes,entries.len(),notify);
    let mut conflicts = Vec::new(); let mut existing = 0;
    for (name, path) in &entries {
        let target = root.join(path); no_links(&target)?;
        if target.exists() {
            if target.is_file() && hash_file_progress(&target,|n|progress.advance(n))? == info.hashes[name] { existing += 1; }
            else { conflicts.push(path.to_string_lossy().into_owned()); }
        }
        progress.file_done();
    }
    let project_file=if info.kind=="Project" {entries.values().find(|p|p.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("uproject"))).map(|p|root.join(p).to_string_lossy().into_owned())}else{None};
    let display_target=if info.kind=="Plugin"{entries.values().find(|p|p.extension().and_then(|e|e.to_str()).is_some_and(|e|e.eq_ignore_ascii_case("uplugin"))).and_then(|p|p.parent()).map(|p|root.join(p)).unwrap_or(root.clone())}else{root.clone()};
    Ok(InstallPreview { project_file, target: display_target.to_string_lossy().into_owned(), files: entries.len(), conflicts, existing, compatible })
}
#[tauri::command]
pub async fn vault_install_preview(id: String, project: String, project_name: Option<String>) -> Result<InstallPreview, String> {
    tauri::async_runtime::spawn_blocking(move || preview(&package(&id)?, &project, project_name.as_deref())).await.map_err(|_| "Проверка установки прервана")?
}
#[tauri::command]
pub async fn vault_prepared_preview(state: tauri::State<'_, VaultState>, id: String, project: String, project_name: Option<String>) -> Result<InstallPreview, String> {
    let info=state.plan.lock().map_err(|_| "Ошибка состояния")?.as_ref().filter(|p|p.info.id==id).map(|p|p.info.clone()).ok_or("Подготовьте материал заново")?;
    tauri::async_runtime::spawn_blocking(move || preview(&info,&project,project_name.as_deref())).await.map_err(|_| "Проверка установки прервана")?
}

fn install(info: &Package, project: &str, project_name: Option<&str>) -> Result<usize, String> {
    install_for_engine(info, project, project_name, None)
}
fn install_for_engine(info: &Package, project: &str, project_name: Option<&str>, engine_version: Option<&str>) -> Result<usize, String> {
    install_for_engine_progress(info,project,project_name,engine_version,&mut |_|{})
}
fn install_for_engine_progress(info:&Package,project:&str,project_name:Option<&str>,engine_version:Option<&str>,notify:&mut dyn FnMut(Progress))->Result<usize,String>{
    if let Some(engine)=engine_version {
        let parts:Vec<_>=engine.split('.').collect();
        if info.kind!="Project" || !(2..=3).contains(&parts.len()) || parts.iter().any(|p|p.is_empty()||!p.bytes().all(|c|c.is_ascii_digit())) {
            return Err("Укажите версию движка в формате 5.8".into());
        }
    }
    let check = preview_progress(info, project, project_name,notify)?;
    if !check.conflicts.is_empty() { return Err("Обнаружены конфликты файлов. Ничего не перезаписано".into()); }
    let (root, entries, _) = install_files(info, project, project_name)?;
    let mut sizes=BTreeMap::new();let mut source_bytes=0u64;let mut copy_bytes=0u64;
    for (name,path) in &entries{
        let source=Path::new(&info.path).join(relative(name)?);no_links(&source)?;
        let bytes=fs::metadata(source).map_err(io_error)?.len();source_bytes=source_bytes.saturating_add(bytes);
        let exists=root.join(path).is_file();copy_bytes=copy_bytes.saturating_add(bytes.saturating_mul(if exists{1}else{2}));sizes.insert(name.clone(),(bytes,exists));
    }
    // Validate every source before creating any project files.
    let mut progress=FileProgress::new("Проверяем файлы Vault",source_bytes,entries.len(),notify);
    for name in entries.keys() {
        if hash_file_progress(&Path::new(&info.path).join(relative(name)?),|n|progress.advance(n))? != info.hashes[name] { return Err("Файл Vault повреждён. Установка отменена".into()); }
        progress.file_done();
    }
    drop(progress);
    if info.kind == "Project" { fs::create_dir_all(root.parent().ok_or("Некорректный каталог")?).map_err(io_error)?; fs::create_dir(&root).map_err(io_error)?; }
    let mut created = Vec::new();
    let mut progress=FileProgress::new("Копируем и проверяем файлы",copy_bytes,entries.len(),notify);
    let result = (|| -> Result<usize, String> {
        for (name, path) in &entries {
            let target = root.join(path); no_links(&target)?;
            if target.exists() {
                if !sizes[name].1{progress.total=progress.total.saturating_sub(sizes[name].0);}
                if target.is_file() && hash_file_progress(&target,|n|progress.advance(n))? == info.hashes[name] { progress.file_done();continue; }
                return Err("Файлы проекта изменились. Установка отменена".into());
            }
            if sizes[name].1{progress.total=progress.total.saturating_add(sizes[name].0);}
            fs::create_dir_all(target.parent().ok_or("Некорректный путь")?).map_err(io_error)?;
            let mut output = OpenOptions::new().write(true).create_new(true).open(&target).map_err(io_error)?;
            created.push(target.clone());
            let mut input = File::open(Path::new(&info.path).join(relative(name)?)).map_err(io_error)?;
            let mut buffer=[0u8;65536];loop{let n=input.read(&mut buffer).map_err(io_error)?;if n==0{break;}output.write_all(&buffer[..n]).map_err(io_error)?;progress.advance(n as u64);}
            output.sync_all().map_err(io_error)?; drop(output);
            if hash_file_progress(&target,|n|progress.advance(n))? != info.hashes[name] { return Err("Проверка скопированного файла не пройдена".into()); }
            if let Some(engine)=engine_version.filter(|_| path.extension().and_then(|e|e.to_str()).is_some_and(|e|e.eq_ignore_ascii_case("uproject"))) {
                let mut descriptor:serde_json::Value=serde_json::from_slice(&fs::read(&target).map_err(io_error)?).map_err(|_|"Не удалось прочитать файл проекта")?;
                let object=descriptor.as_object_mut().ok_or("Некорректный файл проекта")?;
                object.insert("EngineAssociation".into(),serde_json::Value::String(engine.into()));
                fs::write(&target,serde_json::to_vec_pretty(&descriptor).map_err(|_|"Не удалось сохранить версию проекта")?).map_err(io_error)?;
            }
            progress.file_done();
        }
        Ok(created.len())
    })();
    if result.is_err() {
        // Roll back only files exclusively created by this operation; existing files are untouched.
        for path in created.iter().rev() { if no_links(path).is_ok() { let _ = fs::remove_file(path); } }
    }
    drop(progress);
    if result.is_ok(){notify(local_progress("Файлы установлены и проверены",1,1,entries.len(),entries.len()));}
    result
}
fn install_and_register_project(info:&Package,project:&str,project_name:Option<&str>,engine_version:Option<&str>,notify:&mut dyn FnMut(Progress),register:impl FnOnce(&[PathBuf])->Result<(),String>)->Result<usize,String>{
    let projects=if info.kind=="Project" {
        let (root,entries,_)=install_files(info,project,project_name)?;
        entries.values().filter(|p|p.extension().is_some_and(|e|e.eq_ignore_ascii_case("uproject"))).map(|p|root.join(p)).collect::<Vec<_>>()
    }else{Vec::new()};
    let count=install_for_engine_progress(info,project,project_name,engine_version,notify)?;
    if !projects.is_empty(){register(&projects).map_err(|error|format!("Проект установлен, но список не удалось обновить: {error}"))?;}
    Ok(count)
}
#[tauri::command]
pub async fn vault_install(state: tauri::State<'_, VaultState>,webview:tauri::Webview, id: String, project: String, project_name: Option<String>, engine_version: Option<String>,on_progress:Option<tauri::ipc::JavaScriptChannelId>) -> Result<usize, String> {
    let on_progress:Option<tauri::ipc::Channel<Progress>>=on_progress.map(|id|id.channel_on(webview));
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true, Ordering::SeqCst) { return Err("Дождитесь текущей операции Vault".into()); }
    let _busy=Busy(&state.active);
    let (result,project_created)=tauri::async_runtime::spawn_blocking(move || {
        let mut project_created=false;
        let result=(||{
            let info=package(&id)?;
            install_and_register_project(&info,&project,project_name.as_deref(),engine_version.as_deref(),&mut |progress|{if let Some(channel)=&on_progress{let _=channel.send(progress);}},|paths|{project_created=true;crate::project_registry::register_created(paths)})
        })();
        (result,project_created)
    }).await.map_err(|_| "Установка прервана")?;
    if result.is_ok()||project_created{discard_plan(&state);}result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registry_package(root:&Path)->Package{
        let source=root.join("vault");fs::create_dir(&source).unwrap();
        let descriptor=br#"{"FileVersion":3,"EngineAssociation":"5.3"}"#;
        fs::write(source.join("Original.uproject"),descriptor).unwrap();
        Package{artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source:String::new(),asset_id:"fixture".into(),id:"fixture".into(),title:"Fixture Project".into(),kind:"Project".into(),engines:vec![],bytes:descriptor.len() as u64,download_bytes:0,files:1,path:source.to_string_lossy().into(),hashes:BTreeMap::from([("Original.uproject".into(),digest(descriptor))])}
    }
    #[test]
    fn verified_created_project_registers_actual_renamed_descriptor_after_copy(){
        let fixture=crate::project_registry::tests::Fixture::new();let info=registry_package(&fixture.root);
        let target=fixture.root.join("arbitrary destination 空白/new-project");let descriptor=target.join("MyGame.uproject");
        let mut calls=0;
        let n=install_and_register_project(&info,&target.to_string_lossy(),Some("MyGame"),Some("5.8"),&mut |_|{},|paths|{
            calls+=1;assert_eq!(paths,&[descriptor.clone()]);
            assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&descriptor).unwrap()).unwrap()["EngineAssociation"],"5.8");
            fixture.register(paths)
        }).unwrap();
        assert_eq!(n,1);assert_eq!(calls,1);
        let saved=fixture.paths();assert_eq!(saved.len(),1);assert_eq!(crate::project_registry::path_key(&saved[0]),crate::project_registry::path_key(&descriptor));
        let listed=crate::scan_projects_in(&[],&saved);assert_eq!(listed.len(),1);assert_eq!(listed[0].name,"MyGame");
        assert_eq!(fs::read(Path::new(&info.path).join("Original.uproject")).unwrap(),br#"{"FileVersion":3,"EngineAssociation":"5.3"}"#);
    }
    #[test]
    fn failed_creation_never_registers_or_overwrites_an_existing_project(){
        let fixture=crate::project_registry::tests::Fixture::new();let info=registry_package(&fixture.root);
        let target=fixture.root.join("new-project");let descriptor=target.join("MyGame.uproject");let mut calls=0;
        fs::write(Path::new(&info.path).join("Original.uproject"),b"damaged cache").unwrap();
        assert!(install_and_register_project(&info,&target.to_string_lossy(),Some("MyGame"),None,&mut |_|{},|_|{calls+=1;Ok(())}).is_err());
        assert_eq!(calls,0);assert!(!target.exists());assert!(!fixture.record().exists());
        fs::create_dir(&target).unwrap();fs::write(&descriptor,b"original user project").unwrap();
        assert!(install_and_register_project(&info,&target.to_string_lossy(),Some("MyGame"),None,&mut |_|{},|_|{calls+=1;Ok(())}).is_err());
        assert_eq!(calls,0);assert_eq!(fs::read(&descriptor).unwrap(),b"original user project");assert!(!fixture.record().exists());
    }
    #[test]
    fn registry_write_failure_reports_created_project_without_rolling_back_its_files(){
        let fixture=crate::project_registry::tests::Fixture::new();let info=registry_package(&fixture.root);
        let target=fixture.root.join("new-project");
        let result=install_and_register_project(&info,&target.to_string_lossy(),Some("MyGame"),None,&mut |_|{},|_|Err("fixture registry write denied".into()));
        assert!(result.unwrap_err().starts_with("Проект установлен, но список не удалось обновить:"));
        assert_eq!(fs::read(target.join("MyGame.uproject")).unwrap(),br#"{"FileVersion":3,"EngineAssociation":"5.3"}"#);
        assert!(!fixture.record().exists());
    }
    #[test]
    fn filesystem_error_keeps_the_os_cause_and_local_operation_path(){let path=Path::new("X:/Fixture/Vault/file.bin");let error=io_at("Чтение файла",path,std::io::Error::from_raw_os_error(112));assert!(error.contains("os error 112"));assert!(error.contains(&path.to_string_lossy().to_string()));assert!(error.starts_with("Ошибка файловой системы (Чтение файла):"));assert!(io_error(std::io::Error::from_raw_os_error(112)).contains("os error 112"));}
    #[test]
    fn cached_verification_future_fits_async_stack_budget(){
        // Constructing (not polling) this future must not embed the file I/O buffer.
        // It is also nested in engine_install through optional Fab installation.
        let state=VaultState::default();
        let future=hash_download_file(&state,Path::new("X:/Fixture/not-opened"),local_progress("Проверяем файлы Vault",0,1,0,1));
        let bytes=std::mem::size_of_val(&future);println!("cached verification future inline bytes={bytes}");
        assert!(bytes<=4096,"Cached verification future exceeds the 4 KiB async inline-state budget: {bytes} bytes");
    }
    #[test]
    fn install_progress_reports_verified_work_and_preserves_project_on_late_corruption(){
        let root=std::env::temp_dir().join(format!("um-copy-progress-{}",uuid::Uuid::new_v4()));let source=root.join("vault");let project=root.join("project");
        fs::create_dir_all(source.join("Content")).unwrap();fs::create_dir_all(project.join("Content")).unwrap();
        let a=vec![7u8;1024*1024+17];let b=b"second asset";let descriptor=br#"{"EngineAssociation":"5.8","UserSetting":true}"#;
        fs::write(source.join("Content/a.uasset"),&a).unwrap();fs::write(source.join("Content/b.uasset"),b).unwrap();fs::write(project.join("Game.uproject"),descriptor).unwrap();fs::write(project.join("Content/User.uasset"),b"user work").unwrap();
        let info=Package{artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source:String::new(),asset_id:"test".into(),id:"test".into(),title:"test".into(),kind:"Asset".into(),engines:vec!["UE_5.8".into()],bytes:(a.len()+b.len())as u64,download_bytes:0,files:2,path:source.to_string_lossy().into(),hashes:BTreeMap::from([("Content/a.uasset".into(),digest(&a)),("Content/b.uasset".into(),digest(b))])};
        let target=project.join("Game.uproject");let mut events=Vec::new();
        let result=install_for_engine_progress(&info,target.to_str().unwrap(),None,None,&mut |p|{
            if p.phase=="Копируем и проверяем файлы"&&p.files_done==0&&p.done==0{fs::write(source.join("Content/b.uasset"),b"corrupted after validation").unwrap();}events.push(p);
        });
        assert!(result.is_err());assert!(!project.join("Content/a.uasset").exists());assert!(!project.join("Content/b.uasset").exists());
        assert_eq!(fs::read(&target).unwrap(),descriptor);assert_eq!(fs::read(project.join("Content/User.uasset")).unwrap(),b"user work");
        assert!(!events.iter().any(|p|p.phase=="Файлы установлены и проверены"));assert!(events.iter().filter(|p|p.phase=="Копируем и проверяем файлы").all(|p|p.done<p.total));
        fs::write(source.join("Content/b.uasset"),b).unwrap();events.clear();
        assert_eq!(install_for_engine_progress(&info,target.to_str().unwrap(),None,None,&mut |p|events.push(p)).unwrap(),2);
        assert_eq!(hash_file(&project.join("Content/a.uasset")).unwrap(),digest(&a));assert_eq!(fs::read(project.join("Content/b.uasset")).unwrap(),b);
        let copied=events.iter().filter(|p|p.phase=="Копируем и проверяем файлы").last().unwrap();assert_eq!(copied.done,2*info.bytes);assert_eq!(copied.total,2*info.bytes);assert_eq!(copied.files_done,2);
        assert!(events.iter().all(|p|p.network_bytes==0&&!p.can_pause));assert_eq!(events.last().unwrap().phase,"Файлы установлены и проверены");
        fs::remove_file(project.join("Content/a.uasset")).unwrap();events.clear();
        assert_eq!(install_for_engine_progress(&info,target.to_str().unwrap(),None,None,&mut |p|events.push(p)).unwrap(),1);
        let copied=events.iter().filter(|p|p.phase=="Копируем и проверяем файлы").last().unwrap();assert_eq!(copied.total,(2*a.len()+b.len())as u64,"Existing matching file is hashed, not copied again");
        assert_eq!(fs::read(&target).unwrap(),descriptor);assert_eq!(fs::read(project.join("Content/User.uasset")).unwrap(),b"user work");fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupted_source_never_creates_a_new_project_or_reports_installation_complete(){
        let root=std::env::temp_dir().join(format!("um-invalid-source-progress-{}",uuid::Uuid::new_v4()));let source=root.join("vault");fs::create_dir_all(&source).unwrap();fs::write(source.join("Original.uproject"),b"changed").unwrap();
        let info=Package{artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source:String::new(),asset_id:"test".into(),id:"test".into(),title:"test".into(),kind:"Project".into(),engines:vec![],bytes:2,download_bytes:0,files:1,path:source.to_string_lossy().into(),hashes:BTreeMap::from([("Original.uproject".into(),digest(b"{}"))])};
        let target=root.join("NewGame");let mut events=Vec::new();assert!(install_for_engine_progress(&info,target.to_str().unwrap(),Some("NewGame"),None,&mut |p|events.push(p)).is_err());assert!(!target.exists());assert!(!events.iter().any(|p|p.phase=="Копируем и проверяем файлы"||p.phase=="Файлы установлены и проверены"));fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn local_download_verification_can_stop_while_paused_without_mutating_cached_file(){
        let root=std::env::temp_dir().join(format!("um-cached-verification-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();let path=root.join("cached");let data=vec![11u8;2*1024*1024+3];fs::write(&path,&data).unwrap();
        let state=std::sync::Arc::new(VaultState::default());state.paused.store(true,Ordering::SeqCst);let child_state=state.clone();let child_path=path.clone();let bytes=data.len()as u64;
        let task=tokio::spawn(async move{hash_download_file(&child_state,&child_path,local_progress("Проверяем файлы Vault",0,bytes,0,1)).await});
        tokio::time::sleep(Duration::from_millis(30)).await;assert!(!task.is_finished());state.cancel.store(true,Ordering::SeqCst);
        assert!(tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap().is_err());assert_eq!(fs::read(&path).unwrap(),data);
        let fresh=VaultState::default();let (hash,read)=hash_download_file(&fresh,&path,local_progress("Проверяем файлы Vault",0,bytes,0,1)).await.unwrap();assert_eq!(hash,digest(&data));assert_eq!(read,bytes);fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn plugin_targets_and_engine_compatibility(){
        let root=std::env::temp_dir().join(format!("um-plugin-{}",uuid::Uuid::new_v4()));
        let source=root.join("vault");let project=root.join("project");let engine=root.join("UE_5.8");
        fs::create_dir_all(source.join("Nested/Example")).unwrap();fs::create_dir(&project).unwrap();fs::create_dir_all(engine.join("Engine")).unwrap();
        fs::write(source.join("Nested/Example/Example.uplugin"),b"{}").unwrap();
        fs::write(project.join("Game.uproject"),br#"{"EngineAssociation":"5.8"}"#).unwrap();
        let info=Package{artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source:String::new(),asset_id:"test".into(),id:"test".into(),title:"Example".into(),kind:"Plugin".into(),engines:vec!["UE_5.8".into()],bytes:2,download_bytes:0,files:1,path:source.to_string_lossy().into(),hashes:BTreeMap::from([("Nested/Example/Example.uplugin".into(),digest(b"{}"))])};
        let engines=vec![super::super::EngineInstall{name:"UE".into(),version:"5.8".into(),path:engine.to_string_lossy().into(),source:"test".into()}];
        assert_eq!(plugin_engine_root(&info,&engine,&engines).unwrap(),engine.canonicalize().unwrap().join("Engine"));
        assert!(plugin_engine_root(&info,&project,&engines).is_err());
        let marketplace=engine.join("Engine/Plugins/Marketplace");let installed=marketplace.join("Example");fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("Example.uplugin"),b"{}").unwrap();
        assert!(checked_engine_plugin(&marketplace,&marketplace,"Example").is_err());
        assert!(checked_engine_plugin(&marketplace,&project,"Example").is_err());
        assert!(checked_engine_plugin(&marketplace,&installed,"../Example").is_err());
        fs::write(installed.join("Other.uplugin"),b"{}").unwrap();assert!(checked_engine_plugin(&marketplace,&installed,"Example").is_err());fs::remove_file(installed.join("Other.uplugin")).unwrap();
        let target=checked_engine_plugin(&marketplace,&installed,"Example").unwrap();assert_eq!(target.parent(),Some(marketplace.canonicalize().unwrap().as_path()));fs::remove_dir_all(target).unwrap();
        assert!(source.join("Nested/Example/Example.uplugin").exists());assert!(project.join("Game.uproject").exists());
        fs::remove_dir(marketplace).unwrap();fs::remove_dir(engine.join("Engine/Plugins")).unwrap();
        let mut incompatible=info.clone();incompatible.engines=vec!["UE_5.3".into()];assert!(plugin_engine_root(&incompatible,&engine,&engines).is_err());
        assert_eq!(plugin_destination(true,std::ffi::OsStr::new("Example"),Path::new("Example.uplugin")),Path::new("Plugins/Marketplace/Example/Example.uplugin"));
        assert_eq!(install(&info,project.join("Game.uproject").to_str().unwrap(),None).unwrap(),1);
        assert_eq!(fs::read(project.join("Plugins/Example/Example.uplugin")).unwrap(),b"{}");
        assert_eq!(fs::read(source.join("Nested/Example/Example.uplugin")).unwrap(),b"{}");
        fs::remove_file(project.join("Plugins/Example/Example.uplugin")).unwrap();fs::remove_dir(project.join("Plugins/Example")).unwrap();fs::remove_dir(project.join("Plugins")).unwrap();fs::remove_file(project.join("Game.uproject")).unwrap();fs::remove_dir(project).unwrap();
        fs::remove_file(source.join("Nested/Example/Example.uplugin")).unwrap();fs::remove_dir(source.join("Nested/Example")).unwrap();fs::remove_dir(source.join("Nested")).unwrap();fs::remove_dir(source).unwrap();fs::remove_dir(engine.join("Engine")).unwrap();fs::remove_dir(engine).unwrap();fs::remove_dir(root).unwrap();
    }
    #[test]
    fn vault_delete_stays_inside_single_package(){
        let root=std::env::temp_dir().join(format!("um-delete-{}",uuid::Uuid::new_v4()));
        let vault=root.join("vault");let folder=vault.join("material");let project=root.join("project");
        fs::create_dir_all(&folder).unwrap();fs::create_dir(&project).unwrap();
        fs::write(folder.join("asset"),b"vault").unwrap();fs::write(project.join("asset"),b"installed").unwrap();
        assert!(checked_vault_folder(&vault,&vault).is_err());
        assert!(checked_vault_folder(&vault,&project).is_err());
        assert!(checked_vault_folder(&vault,&root).is_err());
        let target=checked_vault_folder(&vault,&folder).unwrap();
        assert_eq!(target.parent(),Some(vault.canonicalize().unwrap().as_path()));
        fs::remove_dir_all(target).unwrap();
        assert!(!folder.exists());assert_eq!(fs::read(project.join("asset")).unwrap(),b"installed");
        fs::remove_file(project.join("asset")).unwrap();fs::remove_dir(project).unwrap();fs::remove_dir(vault).unwrap();fs::remove_dir(root).unwrap();
    }
    #[test]
    fn project_file_name_is_applied_only_to_destination(){
        let root=std::env::temp_dir().join(format!("um-project-name-{}",uuid::Uuid::new_v4()));
        let source=root.join("vault");fs::create_dir_all(&source).unwrap();
        let descriptor=br#"{"FileVersion":3,"EngineAssociation":"5.3"}"#;
        fs::write(source.join("Original.uproject"),descriptor).unwrap();
        let info=Package {artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source:String::new(),asset_id:"test".into(),id:"test".into(),title:"test".into(),kind:"Project".into(),engines:vec![],bytes:descriptor.len() as u64,download_bytes:0,files:1,path:source.to_string_lossy().into(),hashes:BTreeMap::from([("Original.uproject".into(),digest(descriptor))])};
        let destination=root.join("new-project");let target=destination.to_str().unwrap();
        let check=preview(&info,target,Some("MyGame")).unwrap();
        assert_eq!(check.project_file,Some(destination.join("MyGame.uproject").to_string_lossy().into()));
        for invalid in ["","../bad","X:/Fixture/bad","bad.uproject","bad name","CON","1Game"] {assert!(preview(&info,target,Some(invalid)).is_err(),"{invalid}");}
        assert!(!destination.exists());
        assert_eq!(install(&info,target,Some("MyGame")).unwrap(),1);
        assert_eq!(fs::read(destination.join("MyGame.uproject")).unwrap(),descriptor);
        assert!(!destination.join("Original.uproject").exists());
        assert!(source.join("Original.uproject").is_file());
        assert!(install(&info,target,Some("OtherName")).is_err());
        fs::remove_file(destination.join("MyGame.uproject")).unwrap();fs::remove_dir(&destination).unwrap();
        let mut cpp=info.clone();cpp.hashes.insert("Source/Original.Target.cs".into(),digest(b"source"));
        assert!(preview(&cpp,target,Some("Renamed")).is_err());
        assert!(preview(&cpp,target,Some("Original")).is_ok());
        let mut shared=info.clone();shared.engines=vec!["UE_5.3".into()];
        assert!(install_for_engine(&shared,target,Some("NewGame"),Some("invalid")).is_err());
        assert!(!destination.exists());
        assert_eq!(install_for_engine(&shared,target,Some("NewGame"),Some("5.8")).unwrap(),1);
        let copied:serde_json::Value=serde_json::from_slice(&fs::read(destination.join("NewGame.uproject")).unwrap()).unwrap();
        assert_eq!(copied["EngineAssociation"],"5.8");
        assert_eq!(copied["FileVersion"],3);
        assert_eq!(fs::read(source.join("Original.uproject")).unwrap(),descriptor);
        assert!(install_for_engine(&shared,target,Some("NewGame"),Some("5.8")).is_err());
        fs::remove_file(destination.join("NewGame.uproject")).unwrap();fs::remove_dir(&destination).unwrap();
        fs::remove_file(source.join("Original.uproject")).unwrap();fs::remove_dir(source).unwrap();fs::remove_dir(root).unwrap();
    }
    #[test]
    fn rejects_unsafe_windows_paths() {
        for path in ["../a", "X:/Fixture/x", "/x", "a\\..\\x", "a:stream", "NUL.txt", "a/COM1", "a./x", "a//b"] { assert!(relative(path).is_err(), "{path}"); }
        assert!(relative("Content/Тест/a.uasset").is_ok());
    }
    #[test]
    fn chunk_bounds_and_hash() {
        let data=b"sample"; let mut bytes=Vec::new();
        for n in [2986228386u32,3,66,data.len() as u32,0,0,0,0] { bytes.extend(n.to_le_bytes()); }
        bytes.extend(0u64.to_le_bytes()); bytes.push(0); bytes.extend(Sha1::digest(data)); bytes.push(2); bytes.extend((data.len() as u32).to_le_bytes()); bytes.extend(data);
        assert_eq!(decode_chunk(&bytes).unwrap(), data);
        *bytes.last_mut().unwrap()=0; assert!(decode_chunk(&bytes).is_err());
        for n in 0..66 { assert!(decode_chunk(&bytes[..n]).is_err()); }
    }
    #[test]
    fn installation_preserves_conflicts_and_validates_source() {
        let root=std::env::temp_dir().join(format!("um-test-{}", uuid::Uuid::new_v4()));
        let source=root.join("vault"); let project=root.join("project"); fs::create_dir_all(source.join("Content")).unwrap(); fs::create_dir_all(&project).unwrap();
        fs::write(source.join("Content/a.uasset"), b"asset").unwrap(); fs::write(project.join("Demo.uproject"), br#"{"EngineAssociation":"5.8"}"#).unwrap();
        let info=Package { artifact_id:String::new(),selected_engine:String::new(),build_version:String::new(),source: String::new(), asset_id: "test".into(), id: "test".into(), title: "test".into(), kind: "Asset".into(), engines: vec!["UE_5.8".into()], bytes:5, download_bytes:5, files:1, path:source.to_string_lossy().into(), hashes:BTreeMap::from([("Content/a.uasset".into(),digest(b"asset"))]) };
        let dest=project.join("Demo.uproject").to_string_lossy().into_owned();
        assert_eq!(install(&info,&dest,None).unwrap(),1); assert_eq!(install(&info,&dest,None).unwrap(),0);
        fs::write(project.join("Content/a.uasset"),b"user").unwrap(); assert!(install(&info,&dest,None).is_err()); assert_eq!(fs::read(project.join("Content/a.uasset")).unwrap(),b"user");
        fs::write(project.join("Content/a.uasset"),b"asset").unwrap();
        fs::write(source.join("Content/a.uasset"),b"bad").unwrap(); assert!(install(&info,&dest,None).is_err());
        assert_eq!(fs::read(project.join("Content/a.uasset")).unwrap(),b"asset");
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}


#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct EpicVaultIndex { directory:String, items:Vec<Package>, warnings:Vec<String> }
fn epic_index() -> Result<EpicVaultIndex,String> {
    let path=crate::storage::settings().join("epic-vault.json");
    if !path.exists() {return Ok(EpicVaultIndex::default());}
    let bytes=fs::read(path).map_err(io_error)?;
    let mut index:EpicVaultIndex=serde_json::from_slice(&bytes).map_err(|_| "Не удалось прочитать настройки Epic Vault")?;
    for item in &mut index.items {normalize_package_kind(item);}
    Ok(index)
}
fn scan_epic(directory:&str, mut progress:impl FnMut(usize,usize)) -> Result<EpicVaultIndex,String> {
    let root=PathBuf::from(directory); if !root.is_absolute() || !root.is_dir() {return Err("Укажите существующую папку VaultCache полным путём".into());} no_links(&root)?;
    let mut result=EpicVaultIndex {directory:root.to_string_lossy().into_owned(),..Default::default()};
    let entries:Vec<_>=fs::read_dir(&root).map_err(io_error)?.collect::<Result<_,_>>().map_err(io_error)?;
    for (i,entry) in entries.iter().enumerate() {
        progress(i,entries.len());
        let folder=entry.path();let manifest_path=folder.join("manifest");
        if !manifest_path.is_file(){continue;}
        let parsed=(|| -> Result<Package,String> {
            no_links(&manifest_path)?;
            if fs::metadata(&manifest_path).map_err(io_error)?.len()>32*1024*1024 {return Err("Манифест слишком велик".into());}
            let bytes=fs::read(&manifest_path).map_err(io_error)?;
            let manifest=std::panic::catch_unwind(||DownloadManifest::parse(bytes)).ok().flatten().ok_or("Не удалось прочитать манифест")?;
            let fields=manifest.custom_fields.as_ref();let field=|key:&str|fields.and_then(|f|f.get(key)).cloned().unwrap_or_default();
            let data=folder.join("data");no_links(&data)?;
            let mut hashes=BTreeMap::new();let mut total=0u64;let mut seen=BTreeSet::new();
            for file in &manifest.file_manifest_list {
                let relative=relative(&file.filename)?;
                if !seen.insert(file.filename.to_lowercase()) || !valid_hash(&file.file_hash) {return Err("Некорректный список файлов".into());}
                let path=data.join(relative);no_links(&path)?;
                let metadata=fs::metadata(&path).map_err(|_|format!("Отсутствует файл {}",file.filename))?;
                let size:u64=file.size().try_into().map_err(|_|"Некорректный размер файла")?;
                if !metadata.is_file() || metadata.len()!=size {return Err(format!("Не совпадает размер {}",file.filename));}
                total=total.checked_add(size).ok_or("Слишком большой материал")?;
                hashes.insert(file.filename.clone(),file.file_hash.to_ascii_lowercase());
            }
            if hashes.is_empty(){return Err("В материале нет файлов".into());}
            let title=field("Vault.TitleText");let declared=match field("Vault.Type").to_ascii_lowercase().replace(['_',' '],"").as_str(){"plugin"|"engineplugin"=>"Plugin","completeproject"|"project"=>"Project",_=>"Asset"};
            let kind=crate::content_kind::from_paths(declared,hashes.keys());
            Ok(Package {artifact_id:String::new(),selected_engine:String::new(),build_version:manifest.build_version_string.clone(),source:"Epic Games Launcher".into(),asset_id:field("CatalogItemId"),id:digest(format!("epic:{}",data.to_string_lossy().to_lowercase()).as_bytes()),title:if title.is_empty(){manifest.app_name_string.clone()}else{title},kind:kind.into(),engines:field("CompatibleApps").split(',').filter(|s|!s.is_empty()).map(str::to_owned).collect(),bytes:total,download_bytes:0,files:hashes.len(),path:data.to_string_lossy().into_owned(),hashes})
        })();
        match parsed {Ok(package)=>result.items.push(package),Err(error)=>result.warnings.push(format!("{}: {error}",entry.file_name().to_string_lossy()))}
    }
    progress(entries.len(),entries.len());Ok(result)
}
#[tauri::command]
pub fn vault_epic_status()->Result<EpicVaultIndex,String>{epic_index()}
#[derive(Clone, Serialize)]
pub struct EpicScanProgress { done:usize,total:usize }
#[tauri::command]
pub async fn vault_sync_epic(state:tauri::State<'_,VaultState>,epic:tauri::State<'_,super::epic::EpicState>,directory:String,on_progress:tauri::ipc::Channel<EpicScanProgress>)->Result<EpicVaultIndex,String>{
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции Vault".into());} let _busy=Busy(&state.active);
    let mut result=tauri::async_runtime::spawn_blocking(move||scan_epic(&directory,|done,total|{let _=on_progress.send(EpicScanProgress{done,total});})).await.map_err(|_|"Проверка Vault прервана")??;
    let library=super::epic::library_snapshot(&epic).await;
    for item in &mut result.items {
        if let Some(known)=library.iter().find(|i|i.id==item.asset_id) {item.kind=crate::content_kind::from_paths(known.declared_kind(),item.hashes.keys());item.title=known.title.clone();}
    }
    fs::create_dir_all(crate::storage::settings()).map_err(io_error)?;
    let path=crate::storage::settings().join("epic-vault.json");let temp=crate::storage::settings().join(format!("epic-{}.tmp",uuid::Uuid::new_v4()));
    fs::write(&temp,serde_json::to_vec(&result).map_err(|_|"Не удалось сохранить индекс")?).map_err(io_error)?;
    fs::rename(temp,path).map_err(io_error)?;Ok(result)
}
#[cfg(test)]
mod epic_vault_tests {
    use super::*;
    #[test] fn scans_user_epic_vault_read_only(){
        let path=std::env::var("UM_TEST_EPIC_VAULT").unwrap_or_default();if path.is_empty(){return;}
        let index=scan_epic(&path,|_,_|{}).unwrap();
        assert!(!index.items.is_empty(),"{:?}",index.warnings);
        for p in &index.items {assert!(!p.asset_id.is_empty());assert_eq!(p.files,p.hashes.len());}
        println!("Epic Vault: {} packages, {} warnings, {} files",index.items.len(),index.warnings.len(),index.items.iter().map(|p|p.files).sum::<usize>());
        for w in index.warnings {println!("{w}");}
    }
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct UpdateCheck {
    current_id:String, latest_id:String, build_version:String, changed:usize, added:usize, removed:usize,
    bytes:u64, download_bytes:u64, status:String,
}
fn content_diff(old:&BTreeMap<String,String>, new:&BTreeMap<String,String>)->(usize,usize,usize) {
    let normalize=|map:&BTreeMap<String,String>| map.iter().map(|(p,h)|(p.replace('\\',"/").to_lowercase(),h.to_lowercase())).collect::<BTreeMap<_,_>>();
    let old=normalize(old);let new=normalize(new);
    (new.iter().filter(|(p,h)|old.get(*p).is_some_and(|v|v!=*h)).count(),
     new.keys().filter(|p|!old.contains_key(*p)).count(),
     old.keys().filter(|p|!new.contains_key(*p)).count())
}
#[tauri::command]
pub async fn vault_check_update(app:tauri::AppHandle, epic:tauri::State<'_,super::epic::EpicState>, id:String, asset_id:String, artifact:String, engine_version:String)->Result<UpdateCheck,String> {
    let old=package(&id)?;
    if old.asset_id.is_empty() || old.asset_id!=asset_id {return Err("Не удалось однозначно связать пакет с библиотекой Fab".into());}
    if !old.engines.iter().any(|e|e.trim_start_matches("UE_")==engine_version) {return Err("Для проверки выберите одну из исходных версий UE".into());}
    let (item,manifest)=super::epic::download_manifest(&app,&epic,&asset_id,&artifact).await?;
    if !item.versions.iter().any(|v|v.artifact_id==artifact&&v.engines.iter().any(|e|e.trim_start_matches("UE_")==engine_version)&&(v.platforms.is_empty()||v.platforms.iter().any(|p|p.to_lowercase().contains("win")))) {return Err("Нет подходящего пакета Windows для этой версии UE".into());}
    let latest=make_plan(item,&artifact,manifest)?.info;
    let (changed,added,removed)=content_diff(&old.hashes,&latest.hashes);
    let same=changed+added+removed==0;
    let cached=vault_list()?.into_iter().find(|p|p.asset_id==asset_id&&content_diff(&p.hashes,&latest.hashes)==(0,0,0));
    Ok(UpdateCheck{current_id:id,latest_id:cached.as_ref().map(|p|p.id.clone()).unwrap_or(latest.id),build_version:latest.build_version,changed,added,removed,bytes:latest.bytes,download_bytes:latest.download_bytes,status:if same{"current"}else if cached.is_some(){"downloaded"}else{"available"}.into()})
}
#[cfg(test)]
mod update_tests {
 use super::*;
 #[test] fn detects_changed_added_and_removed_files() {
  let old=BTreeMap::from([("Content/A.uasset".into(),"aa".into()),("Content/Old.uasset".into(),"bb".into())]);
  let new=BTreeMap::from([("Content/A.uasset".into(),"cc".into()),("Content/New.uasset".into(),"dd".into())]);
  assert_eq!(content_diff(&old,&new),(1,1,1));
 }
 #[test] fn ignores_windows_path_spelling_and_hash_case() {
  assert_eq!(content_diff(&BTreeMap::from([("Content\\A.uasset".into(),"AB".into())]),&BTreeMap::from([("content/a.uasset".into(),"ab".into())])),(0,0,0));
 }
 #[test] fn legacy_package_metadata_remains_readable() {
  let old=r#"{"id":"old","title":"Asset","kind":"Asset","engines":["UE_5.8"],"bytes":1,"downloadBytes":1,"files":1,"path":"test","hashes":{}}"#;
  let p:Package=serde_json::from_str(old).unwrap();
  assert!(p.artifact_id.is_empty());assert!(p.selected_engine.is_empty());
 }
}



pub(crate) async fn install_fab_locked(app:&tauri::AppHandle,epic:&crate::epic::EpicState,state:&VaultState,root:&Path,version:&str,on_progress:tauri::ipc::Channel<Progress>)->Result<(),String>{
 // The engine installer owns the Vault busy guard for this entire operation.
 let (item,manifest)=crate::epic::fab_plugin_package(app,epic,version).await?;
 let artifact=format!("FabPlugin_{version}");
 let mut plan=make_plan(item,&artifact,manifest)?;plan.info.selected_engine=version.into();
 let id=plan.info.id.clone();
 *state.plan.lock().map_err(|_|"Ошибка состояния загрузки")?=Some(plan);
 state.cancel.store(false,Ordering::SeqCst);
 let install_channel=on_progress.clone();
 let info=download_locked(state,&id,Some(on_progress)).await?;
 let destination=root.to_string_lossy().into_owned();
 tauri::async_runtime::spawn_blocking(move||{crate::engine::install::idle(Path::new(&destination))?;install_for_engine_progress(&info,&destination,None,None,&mut |progress|{let _=install_channel.send(progress);})}).await.map_err(|_|"Установка Fab прервана")??;
 *state.plan.lock().map_err(|_|"Ошибка состояния загрузки")?=None;
 Ok(())
}

pub(crate) fn discard_plan(state:&VaultState){if let Ok(mut plan)=state.plan.lock(){*plan=None;}crate::vault_queue::wake();}
#[tauri::command]
pub fn vault_discard(state:tauri::State<'_,VaultState>)->Result<(),String>{
 discard_idle(&state)
}
fn discard_idle(state:&VaultState)->Result<(),String>{
 if state.active.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){return Err("Дождитесь текущей операции Vault".into());}
 let _busy=Busy(&state.active);discard_plan(&state);Ok(())
}

pub(crate) fn queue_plan_reserved(state:&VaultState)->bool{state.plan.lock().map(|p|p.is_some()).unwrap_or(true)}
pub(crate) fn queue_lease<'a>(state:&'a VaultState,engine:&crate::engine::EngineState)->Option<Busy<'a>>{
 if engine.active.load(Ordering::SeqCst)||queue_plan_reserved(state){return None;}
 state.active.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).ok()?;
 let lease=Busy(&state.active);
 if engine.active.load(Ordering::SeqCst)||queue_plan_reserved(state){return None;}Some(lease)
}
fn queue_accept_package(info:&Package,prepared:&mut impl FnMut(&Package)->Result<(),String>)->Result<(),String>{
 prepared(info)?;
 if transfers::has_saved_task(&info.id)?{return Err("Продолжите сохранённую загрузку в списке задач.".into());}Ok(())
}
pub(crate) async fn queue_download(app:&tauri::AppHandle,epic:&crate::epic::EpicState,state:&VaultState,request:&crate::vault_queue::Request,account:&str,mut prepared:impl FnMut(&Package)->Result<(),String>,sink:std::sync::Arc<dyn Fn(Progress)+Send+Sync>)->Result<Package,String>{
 // Caller owns the same Vault lease as manual downloads and engine installation.
 let plan=prepare_locked(app,epic,&request.asset_id,&request.artifact_id,&request.selected_engine,Some(account)).await?;
 queue_accept_package(&plan.info,&mut prepared)?;
 if !Path::new(&plan.info.path).exists(){let required=plan.info.bytes.checked_add(plan.info.download_bytes).and_then(|n|n.checked_add(128*1024*1024)).ok_or("Материал слишком велик")?;if crate::engine::free(Path::new(&plan.info.path))?<required{return Err("Недостаточно свободного места.".into());}}
 let id=plan.info.id.clone();*state.plan.lock().map_err(|_|"Ошибка состояния загрузки")?=Some(plan);
 *state.progress_sink.lock().map_err(|_|"Ошибка состояния загрузки")?=Some(sink);
 let result=download_locked(state,&id,None).await;
 discard_plan(state);result
}

#[cfg(test)]mod queue_tests{
 use super::*;
 fn info()->Package{let id=digest(uuid::Uuid::new_v4().as_bytes());Package{artifact_id:"synthetic-artifact".into(),selected_engine:"5.7".into(),build_version:"build".into(),source:String::new(),asset_id:"synthetic-asset".into(),id:id.clone(),title:"Synthetic".into(),kind:"Asset".into(),engines:vec!["5.7".into()],bytes:0,download_bytes:0,files:0,path:crate::storage::vault().join(id).to_string_lossy().into_owned(),hashes:BTreeMap::new()}}
 #[test]fn queue_uses_existing_exclusive_lease_and_preserves_manual_prepared_plan(){
  let vault=VaultState::default();let engine=crate::engine::EngineState::default();
  engine.active.store(true,Ordering::SeqCst);assert!(queue_lease(&vault,&engine).is_none());assert!(!vault.active.load(Ordering::SeqCst));engine.active.store(false,Ordering::SeqCst);
  vault.active.store(true,Ordering::SeqCst);assert!(queue_lease(&vault,&engine).is_none());assert!(vault.active.load(Ordering::SeqCst));vault.active.store(false,Ordering::SeqCst);
  let plan=Plan{info:info(),intent:Some(transfers::Intent{mode:"install".into(),project:"X:/Fixture/Game.uproject".into(),project_name:"Game".into(),engine_version:"5.7".into(),update_id:None}),files:BTreeMap::new()};let before=serde_json::to_vec(&plan).unwrap();*vault.plan.lock().unwrap()=Some(plan);assert!(queue_lease(&vault,&engine).is_none());assert_eq!(serde_json::to_vec(vault.plan.lock().unwrap().as_ref().unwrap()).unwrap(),before);discard_plan(&vault);
  let lease=queue_lease(&vault,&engine).unwrap();assert!(queue_lease(&vault,&engine).is_none());
  // A late close of a manual wizard must not erase a claimed queue download.
  let queued=Plan{info:info(),intent:None,files:BTreeMap::new()};let queued_id=queued.info.id.clone();*vault.plan.lock().unwrap()=Some(queued);assert!(discard_idle(&vault).is_err());assert_eq!(vault.plan.lock().unwrap().as_ref().unwrap().info.id,queued_id);drop(lease);assert!(!vault.active.load(Ordering::SeqCst));discard_idle(&vault).unwrap();assert!(queue_lease(&vault,&engine).is_some());
 }
 #[test]fn queue_cannot_replace_an_existing_partial_plan_or_its_intent(){
  let info=info();let base=crate::storage::settings().join("VaultTasks");fs::create_dir_all(&base).unwrap();let json=base.join(format!("{}.json",info.id));let zlib=base.join(format!("{}.zlib",info.id));let metadata=b"existing project intent must remain";let data=b"existing private resumable plan";fs::write(&json,metadata).unwrap();fs::write(&zlib,data).unwrap();let mut recorded=None;
  assert_eq!(queue_accept_package(&info,&mut |p|{recorded=Some(p.id.clone());Ok(())}).unwrap_err(),"Продолжите сохранённую загрузку в списке задач.");assert_eq!(recorded,Some(info.id));assert_eq!(fs::read(&json).unwrap(),metadata);assert_eq!(fs::read(&zlib).unwrap(),data);fs::remove_file(json).unwrap();fs::remove_file(zlib).unwrap();
 }
}

#[cfg(test)]mod cache_metadata_tests{
 use super::*;
 const DATA:&[u8]=b"synthetic verified asset bytes";
 static PROVENANCE:Mutex<()>=Mutex::new(());
 struct Fixture{root:PathBuf,vault:PathBuf}
 impl Fixture{
  fn new()->Self{
   let temp=std::env::temp_dir();assert!(temp.is_absolute(),"Fixture temporary directory must be absolute");
   let root=temp.join(format!("um-vault-cache-test-{}-{}",std::process::id(),uuid::Uuid::new_v4()));let vault=root.join("Vault");fs::create_dir_all(&vault).unwrap();Self{root,vault}
  }
  fn plan(&self,artifact:&str,engines:&[&str],selected:&str)->Plan{
   let hashes=BTreeMap::from([("Content/Demo.uasset".into(),digest(DATA))]);
   let fingerprint=serde_json::to_string(&hashes).unwrap();let id=digest(format!("fixture-asset\n{artifact}\nfixture-build\n{fingerprint}").as_bytes());
   Plan{intent:None,files:BTreeMap::new(),info:Package{artifact_id:artifact.into(),selected_engine:selected.into(),build_version:"fixture-build".into(),source:String::new(),asset_id:"fixture-asset".into(),id:id.clone(),title:format!("Fresh {artifact}"),kind:"Asset".into(),engines:engines.iter().map(|v|(*v).into()).collect(),bytes:DATA.len() as u64,download_bytes:DATA.len() as u64,files:1,path:self.vault.join(id).to_string_lossy().into_owned(),hashes}}
  }
  fn own(&self,info:&Package){let root=Path::new(&info.path);assert!(root.starts_with(&self.root));fs::create_dir_all(root.join("Content")).unwrap();fs::write(root.join("Content/Demo.uasset"),DATA).unwrap();fs::write(root.join("package.um.json"),serde_json::to_vec_pretty(info).unwrap()).unwrap();}
  fn project_plan(&self,kind:&str)->Plan{
   let mut plan=self.plan("project-artifact",&["5.8"],"5.8");plan.info.kind=kind.into();plan.info.hashes=project_files().into_iter().map(|(name,bytes)|(name.into(),digest(bytes))).collect();plan.info.files=plan.info.hashes.len();plan.info.bytes=project_files().iter().map(|(_,bytes)|bytes.len()as u64).sum();
   let fingerprint=serde_json::to_string(&plan.info.hashes).unwrap();plan.info.id=digest(format!("fixture-asset\nproject-artifact\nfixture-build\n{fingerprint}").as_bytes());plan.info.path=self.vault.join(&plan.info.id).to_string_lossy().into_owned();plan
  }
  fn own_project(&self,info:&Package){let root=Path::new(&info.path);assert!(root.starts_with(&self.root));for(name,bytes)in project_files(){let target=root.join(name);fs::create_dir_all(target.parent().unwrap()).unwrap();fs::write(target,bytes).unwrap();}fs::write(root.join("package.um.json"),serde_json::to_vec_pretty(info).unwrap()).unwrap();}
  fn external(&self,fresh:&Package,engines:&[&str])->Package{
   let mut p=fresh.clone();p.id=digest(uuid::Uuid::new_v4().as_bytes());p.source=EPIC_CACHE_SOURCE.into();p.path=self.root.join("EpicCache").join(&p.id).to_string_lossy().into_owned();p.artifact_id.clear();p.selected_engine.clear();p.title="Cached Epic title".into();p.engines=engines.iter().map(|e|(*e).into()).collect();fs::create_dir_all(Path::new(&p.path).join("Content")).unwrap();fs::write(Path::new(&p.path).join("Content/Demo.uasset"),DATA).unwrap();p
  }
  fn no_task_or_stage(&self,id:&str){assert!(!self.vault.join(".partial").join(id).exists());let tasks=crate::storage::settings().join("VaultTasks");for ext in["json","zlib"]{assert!(!tasks.join(format!("{id}.{ext}")).exists());}}
  fn compatible(&self,info:&Package,version:&str)->bool{let path=self.root.join(format!("Demo-{version}.uproject"));fs::write(&path,serde_json::to_vec(&serde_json::json!({"EngineAssociation":version})).unwrap()).unwrap();preview(info,&path.to_string_lossy(),None).unwrap().compatible}
 }
 impl Drop for Fixture{fn drop(&mut self){
  let mut files=0;let mut bytes=0;for e in walkdir::WalkDir::new(&self.root).follow_links(false){let e=e.unwrap();if e.file_type().is_file(){files+=1;bytes+=e.metadata().unwrap().len();}}
  assert_eq!(self.root.parent(),Some(std::env::temp_dir().as_path()));assert!(self.root.file_name().unwrap().to_string_lossy().starts_with(&format!("um-vault-cache-test-{}-",std::process::id())));
  fs::remove_dir_all(&self.root).unwrap();
  if let Ok(path)=std::env::var("UM_CACHE_TEST_PROVENANCE"){let _guard=PROVENANCE.lock().unwrap();let path=PathBuf::from(path);assert!(path.is_absolute());let mut file=OpenOptions::new().create(true).append(true).open(path).unwrap();writeln!(file,"{}",serde_json::json!({"pid":std::process::id(),"root":self.root,"files":files,"bytes":bytes,"removed":!self.root.exists(),"purpose":"synthetic cache metadata regression fixture"})).unwrap();}
 }}
 fn project_files()->[(&'static str,&'static [u8]);3]{[("DMD_MSS.uproject",br#"{"FileVersion":3,"EngineAssociation":"5.8"}"#),("Config/DefaultEngine.ini",b"[/Script/EngineSettings.GameMapsSettings]"),("Content/MSS/Demo.uasset",DATA)]}
 fn library_item(display_kind:&str,base_kind:&str)->crate::epic::LibraryItem{serde_json::from_value(serde_json::json!({"id":"fixture-asset","title":"Selected project","kind":display_kind,"catalogKind":base_kind,"engineVersions":["5.8"],"categories":[],"source":"Fixture","namespace":"fixture","versions":[{"artifactId":"project-artifact","engines":["5.8"],"platforms":["Windows"]}]})).unwrap()}
 fn selected_manifest(paths:&[&str])->DownloadManifest{DownloadManifest{build_version_string:"fixture-build".into(),file_manifest_list:paths.iter().map(|name|FileManifestList{filename:(*name).into(),file_hash:digest(b""),file_chunk_parts:vec![]}).collect(),..DownloadManifest::default()}}
 #[test]fn selected_manifest_promotes_generic_catalogue_without_changing_identity(){
  let manifest=selected_manifest(&["DMD_MSS.uproject","Config/DefaultEngine.ini","Content/MSS/Demo.uasset"]);let plan=make_plan(library_item("Asset","Asset"),"project-artifact",manifest.clone()).unwrap();let declared=make_plan(library_item("Project","Project"),"project-artifact",manifest).unwrap();
  assert_eq!(plan.info.kind,"Project");assert_eq!(plan.info.id,declared.info.id);assert_eq!(plan.info.hashes,declared.info.hashes);assert_eq!(plan.info.artifact_id,"project-artifact");assert_eq!(plan.info.build_version,"fixture-build");assert_eq!(plan.info.engines,vec!["5.8"]);assert_eq!(plan.info.files,3);
 }
 #[test]fn another_version_catalogue_hint_does_not_promote_content_only_selection(){
  let plan=make_plan(library_item("Project","Asset"),"project-artifact",selected_manifest(&["Content/Only.uasset"])).unwrap();assert_eq!(plan.info.kind,"Asset");
 }
 #[test]fn old_downloaded_asset_record_is_project_on_read_without_rewriting_it(){
  let f=Fixture::new();let old=f.project_plan("Asset");f.own_project(&old.info);let record=Path::new(&old.info.path).join("package.um.json");let before=fs::read(&record).unwrap();let loaded=package_at(&f.vault,&old.info.id).unwrap();
  assert_eq!(loaded.kind,"Project");assert_eq!(loaded.id,old.info.id);assert_eq!(loaded.hashes,old.info.hashes);assert_eq!(fs::read(&record).unwrap(),before);
  let destination=f.root.join("NewProject");let preview=preview(&loaded,&destination.to_string_lossy(),Some("NewProject")).unwrap();assert_eq!(preview.files,3);assert_eq!(preview.project_file,Some(destination.join("NewProject.uproject").to_string_lossy().into_owned()));assert!(!destination.exists());
  let existing=f.root.join("Existing.uproject");fs::write(&existing,b"existing user project sentinel").unwrap();assert!(install_files(&loaded,&existing.to_string_lossy(),Some("NewProject")).unwrap_err().contains("ещё не существующий"));assert_eq!(fs::read(existing).unwrap(),b"existing user project sentinel");
 }
 #[tokio::test]async fn corrected_cached_type_reuses_verified_files_without_redownload(){
  let f=Fixture::new();let old=f.project_plan("Asset");f.own_project(&old.info);let fresh=f.project_plan("Project");assert!(same_cached_files(&old.info,&fresh.info));let state=VaultState::default();*state.plan.lock().unwrap()=Some(fresh.clone());
  let result=download_locked_from(&state,&fresh.info.id,None,&f.vault,&||Ok(vec![])).await.unwrap();assert_eq!(result.kind,"Project");assert_eq!(result.id,old.info.id);assert_eq!(result.artifact_id,"project-artifact");assert_eq!(result.selected_engine,"5.8");assert_eq!(result.hashes,old.info.hashes);let stored:Package=serde_json::from_slice(&fs::read(Path::new(&result.path).join("package.um.json")).unwrap()).unwrap();assert_eq!(stored.kind,"Project");
  for(name,bytes)in project_files(){assert_eq!(fs::read(Path::new(&result.path).join(name)).unwrap(),bytes);}f.no_task_or_stage(&result.id);
 }
 #[test]fn project_install_intent_requires_new_folder_but_asset_install_is_unchanged(){
  let f=Fixture::new();let mut plan=f.project_plan("Asset");let intent=transfers::Intent{mode:"install".into(),project:"X:/Fixture/Game.uproject".into(),project_name:"Game".into(),engine_version:"5.8".into(),update_id:None};plan.intent=Some(intent.clone());assert!(!project_intent_needs_destination(&plan.info,&intent));assert!(normalize_plan_kind(&mut plan));assert_eq!(plan.info.kind,"Project");assert!(plan.intent.is_none());assert!(project_intent_needs_destination(&plan.info,&intent));
  let mut fresh=intent;fresh.project="X:/Fixture/NewProject".into();assert!(!project_intent_needs_destination(&plan.info,&fresh));plan.intent=Some(fresh);assert!(!normalize_plan_kind(&mut plan));assert!(plan.intent.is_some());
 }
 async fn cached(f:&Fixture,plan:Plan,index:impl Fn()->Result<Vec<Package>,String>+Sync)->Result<Package,String>{let state=VaultState::default();let id=plan.info.id.clone();*state.plan.lock().unwrap()=Some(plan);download_locked_from(&state,&id,None,&f.vault,&index).await}

 #[tokio::test]async fn different_artifact_same_bytes_never_reuses_old_own_compatibility(){
  let f=Fixture::new();let old=f.plan("artifact-a",&["5.6"],"5.6");f.own(&old.info);let fresh=f.plan("artifact-b",&["5.7"],"5.7");assert_ne!(old.info.id,fresh.info.id);
  let selected=select_prepared_cache(fresh.clone(),&f.vault,vec![old.info.clone()]);assert_eq!(selected.info.id,fresh.info.id);assert_eq!(selected.info.path,fresh.info.path);
  f.own(&fresh.info);let result=cached(&f,selected,||Ok(vec![])).await.unwrap();assert_eq!(result.artifact_id,"artifact-b");assert!(f.compatible(&result,"5.7"));assert!(!f.compatible(&result,"5.6"));assert_eq!(package_at(&f.vault,&old.info.id).unwrap().engines,vec!["5.6"]);
 }
 #[tokio::test]async fn same_canonical_cache_refreshes_publisher_compatibility_after_hashes(){
  let f=Fixture::new();let old=f.plan("shared-artifact",&["5.6"],"5.6");f.own(&old.info);let fresh=f.plan("shared-artifact",&["5.6","5.7"],"5.7");assert_eq!(old.info.id,fresh.info.id);
  let selected=select_prepared_cache(fresh.clone(),&f.vault,vec![]);let result=cached(&f,selected,||Ok(vec![])).await.unwrap();assert_eq!(result.engines,fresh.info.engines);let reloaded=package_at(&f.vault,&result.id).unwrap();assert_eq!(reloaded.engines,fresh.info.engines);assert!(f.compatible(&reloaded,"5.7"));
 }
 #[tokio::test]async fn shared_artifact_retains_new_selected_engine_in_result_and_record(){
  let f=Fixture::new();let old=f.plan("shared-artifact",&["5.6","5.7"],"5.6");f.own(&old.info);let mut fresh=old.clone();fresh.info.selected_engine="5.7".into();fresh.info.title="Fresh publisher title".into();let result=cached(&f,select_prepared_cache(fresh,&f.vault,vec![]),||Ok(vec![])).await.unwrap();assert_eq!(result.selected_engine,"5.7");let reloaded=package_at(&f.vault,&result.id).unwrap();assert_eq!(reloaded.selected_engine,"5.7");assert_eq!(reloaded.title,"Fresh publisher title");
 }
 #[tokio::test]async fn compatible_external_cache_keeps_physical_identity_without_writes(){
  let f=Fixture::new();let fresh=f.plan("requested-artifact",&["5.7","5.8"],"5.7");let external=f.external(&fresh.info,&[" UE_5.7 ","ue_5.8","5.9"]);let index_path=f.root.join("external-index.json");let index_bytes=serde_json::to_vec(&external).unwrap();fs::write(&index_path,&index_bytes).unwrap();let physical=Path::new(&external.path).join("Content/Demo.uasset");let selected=select_prepared_cache(fresh.clone(),&f.vault,vec![external.clone()]);assert_eq!(selected.info.id,external.id);
  let result=cached(&f,selected,||Ok(vec![external.clone()])).await.unwrap();assert_eq!(result.path,external.path);assert_eq!(result.source,EPIC_CACHE_SOURCE);assert_eq!(result.artifact_id,fresh.info.artifact_id);assert_eq!(result.build_version,fresh.info.build_version);assert_eq!(result.title,fresh.info.title);assert_eq!(result.selected_engine,"5.7");assert_eq!(result.engines,fresh.info.engines);assert!(f.compatible(&result,"5.8"));assert_eq!(fs::read(&physical).unwrap(),DATA);assert_eq!(fs::read(index_path).unwrap(),index_bytes);assert!(!Path::new(&external.path).join("package.um.json").exists());assert!(!f.vault.join(&fresh.info.id).exists());f.no_task_or_stage(&external.id);
 }
 #[tokio::test]async fn external_must_cover_all_requested_engines_not_only_selection(){
  let f=Fixture::new();let fresh=f.plan("requested",&["5.7","5.8"],"5.7");let external=f.external(&fresh.info,&["5.7"]);let selected=select_prepared_cache(fresh.clone(),&f.vault,vec![external.clone()]);assert_eq!(selected.info.id,fresh.info.id);assert_eq!(selected.info.path,fresh.info.path);assert!(selected.info.source.is_empty());f.own(&selected.info);let result=cached(&f,selected,||Ok(vec![external.clone()])).await.unwrap();assert_eq!(result.id,fresh.info.id);assert!(f.compatible(&result,"5.8"));assert_eq!(fs::read(Path::new(&external.path).join("Content/Demo.uasset")).unwrap(),DATA);
 }
 #[tokio::test]async fn disappeared_external_cache_fails_before_staging_or_task_save(){
  let f=Fixture::new();let fresh=f.plan("requested",&["5.7"],"5.7");let external=f.external(&fresh.info,&["5.7"]);let selected=select_prepared_cache(fresh,&f.vault,vec![external.clone()]);fs::remove_dir_all(&external.path).unwrap();assert!(cached(&f,selected,||Ok(vec![external.clone()])).await.err().unwrap().contains("Epic Vault"));assert!(!Path::new(&external.path).exists());f.no_task_or_stage(&external.id);
 }
 #[tokio::test]async fn external_index_revalidated_after_hashing_without_writes(){
  let f=Fixture::new();let fresh=f.plan("requested",&["5.7","5.8"],"5.7");let external=f.external(&fresh.info,&["5.7","5.8"]);let selected=select_prepared_cache(fresh,&f.vault,vec![external.clone()]);let calls=std::sync::atomic::AtomicUsize::new(0);let result=cached(&f,selected,||{let mut p=external.clone();if calls.fetch_add(1,Ordering::SeqCst)>0{p.engines=vec!["5.7".into()];}Ok(vec![p])}).await;assert!(result.is_err());assert_eq!(calls.load(Ordering::SeqCst),2);assert_eq!(fs::read(Path::new(&external.path).join("Content/Demo.uasset")).unwrap(),DATA);assert!(!Path::new(&external.path).join("package.um.json").exists());f.no_task_or_stage(&external.id);
 }
 #[tokio::test]async fn corrupt_own_cache_leaves_previous_package_record_byte_exact(){
  let f=Fixture::new();let old=f.plan("shared",&["5.6"],"5.6");f.own(&old.info);let record=Path::new(&old.info.path).join("package.um.json");let before=fs::read(&record).unwrap();fs::write(Path::new(&old.info.path).join("Content/Demo.uasset"),b"corrupt").unwrap();let fresh=f.plan("shared",&["5.6","5.7"],"5.7");assert!(cached(&f,select_prepared_cache(fresh,&f.vault,vec![]),||Ok(vec![])).await.is_err());assert_eq!(fs::read(record).unwrap(),before);f.no_task_or_stage(&old.info.id);
 }
 #[cfg(windows)]#[tokio::test]async fn locked_own_record_preserves_old_bytes_and_removes_only_own_failed_temporary(){
  use std::os::windows::fs::OpenOptionsExt;
  let f=Fixture::new();let old=f.plan("shared",&["5.6"],"5.6");f.own(&old.info);let directory=Path::new(&old.info.path);let record=directory.join("package.um.json");let before=fs::read(&record).unwrap();let siblings=||fs::read_dir(directory).unwrap().map(|e|e.unwrap().file_name()).collect::<BTreeSet<_>>();let before_names=siblings();
  let held=OpenOptions::new().read(true).share_mode(1).open(&record).unwrap();let fresh=f.plan("shared",&["5.6","5.7"],"5.7");assert!(cached(&f,select_prepared_cache(fresh,&f.vault,vec![]),||Ok(vec![])).await.is_err());assert_eq!(fs::read(&record).unwrap(),before);assert_eq!(siblings(),before_names);drop(held);f.no_task_or_stage(&old.info.id);
 }
}

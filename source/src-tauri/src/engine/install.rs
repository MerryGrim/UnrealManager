use super::*;
use sha1::{Digest, Sha1};
use tauri::Manager;
#[path="verification.rs"]
mod verification;
use std::{
    io::Write,
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    phase: String,
    done: u64,
    total: u64,
    percent: u64,
    network_bytes: u64,
    processed_bytes: u64,
    total_bytes: u64,
    files_done: u64,
    files_total: u64,
    paused: bool,
    can_pause: bool,
    #[serde(skip_serializing_if="Option::is_none")]
    current_file: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    files_per_second: Option<f64>,
    #[serde(skip_serializing_if="Option::is_none")]
    read_bytes_per_second: Option<u64>,
}
fn fab_progress(value:&serde_json::Value,engine_network:u64,received:&std::sync::atomic::AtomicU64)->Progress {
    let done=value["done"].as_u64().unwrap_or(0);let total=value["total"].as_u64().unwrap_or(0);
    let highest=received.fetch_max(value["networkBytes"].as_u64().unwrap_or(0),Ordering::SeqCst).max(value["networkBytes"].as_u64().unwrap_or(0));
    let phase=value["phase"].as_str().filter(|p|!p.is_empty()).map(|p|format!("Fab: {p}")).unwrap_or_else(||"Устанавливаем Fab".into());
    Progress{phase,done,total,percent:done.saturating_mul(100)/total.max(1),network_bytes:engine_network.saturating_add(highest),processed_bytes:value["processedBytes"].as_u64().unwrap_or(done),total_bytes:value["totalBytes"].as_u64().unwrap_or(total),files_done:value["filesDone"].as_u64().unwrap_or(0),files_total:value["filesTotal"].as_u64().unwrap_or(0),paused:false,can_pause:false,..Default::default()}
}
pub(crate) struct Busy<'a>(pub(crate) &'a AtomicBool);
struct PauseScope<'a>(&'a EngineState);
impl Drop for PauseScope<'_>{fn drop(&mut self){self.0.pause_available.store(false,Ordering::SeqCst);self.0.paused.store(false,Ordering::SeqCst);}}
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
        crate::vault_queue::wake();
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Operation {
    pub(super) name: String,
    pub(super) old_hash: Option<String>,
    pub(super) new_hash: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    root: String,
    build: String,
    operations: Vec<Operation>,
    status: String,
    item: Option<(PathBuf, serde_json::Value, serde_json::Value)>,
    #[serde(default)]
    launcher: Option<(PathBuf, serde_json::Value, serde_json::Value)>,
    #[serde(default)]
    launcher_warning: Option<String>,
}
const REGISTRATION_PHASE:&str="Регистрируем движок";
const ROLLBACK_PHASE:&str="Восстанавливаем прежние файлы";
const REGISTRATION_WARNING:&str="Общий список Epic не обновлён. Установленный движок доступен в Unreal Manager.";
const TASK_VALIDATION_PHASE:&str="Проверяем сохранённую задачу";
const CACHE_INVENTORY_PHASE:&str="Проверяем наличие блоков";
const CACHE_VALIDATION_PHASE:&str="Проверяем сохранённые блоки";
pub(super) fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha1::digest(b))
}
struct BlockCache {
    blocks: BTreeMap<String, Vec<u8>>,
    order: std::collections::VecDeque<String>,
    bytes: usize,
    limit: usize,
}
impl BlockCache {
    fn new(limit: usize) -> Self {
        Self { blocks: BTreeMap::new(), order: Default::default(), bytes: 0, limit }
    }
    fn get(&mut self, stage: &Path, guid: &str) -> Result<&[u8], String> {
        if !guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') || guid.is_empty() {
            return Err("Некорректный идентификатор блока".into());
        }
        if !self.blocks.contains_key(guid) {
            let path = stage.join("chunks").join(guid);
            no_links(&path)?;
            let data = crate::vault::decode_chunk(&fs::read(path).map_err(io)?)?;
            if data.len() > self.limit { return Err("Блок слишком велик".into()); }
            while self.bytes + data.len() > self.limit {
                let oldest = self.order.pop_front().ok_or("Ошибка кэша блоков")?;
                if let Some(old) = self.blocks.remove(&oldest) { self.bytes -= old.len(); }
            }
            self.bytes += data.len();
            self.blocks.insert(guid.into(), data);
        } else {
            self.order.retain(|s| s != guid);
        }
        self.order.push_back(guid.into());
        Ok(&self.blocks[guid])
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    no_links(path)?;
    fs::create_dir_all(path.parent().ok_or("Путь файла")?).map_err(io)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io)?;
    f.write_all(bytes).map_err(io)?;
    f.sync_all().map_err(io)
}
pub(crate) fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    no_links(path)?;
    let temp = path.with_extension(format!("um-{}", uuid::Uuid::new_v4()));
    write_new(&temp, bytes)?;
    let result=fs::rename(&temp,path).map_err(io);
    if result.is_err(){let _=fs::remove_file(&temp);}
    result
}
fn critical_engine_process(name:&str)->bool{
    ["unrealeditor.exe","unrealeditor-cmd.exe","unrealbuildtool.exe","shadercompileworker.exe","dotnet.exe"].iter().any(|n|name.eq_ignore_ascii_case(n))
}
fn process_path_key(path:&Path)->Result<String,String>{
    let text=path.to_str().ok_or("Не удалось проверить запущенные приложения")?.replace('/',"\\").to_lowercase();
    let text=if let Some(unc)=text.strip_prefix("\\\\?\\unc\\"){format!("\\\\{unc}")}else{text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()};
    let absolute=text.starts_with("\\\\")||(text.as_bytes().get(1)==Some(&b':')&&text.as_bytes().get(2)==Some(&b'\\'));
    if !absolute||text.starts_with("\\\\.\\")||text.split('\\').any(|part|part=="."||part==".."){return Err("Не удалось проверить запущенные приложения".into());}
    Ok(text.trim_end_matches('\\').to_string())
}
fn guard_engine_process(root:&Path,name:&str,image:Option<&Path>)->Result<(),String>{
    if !critical_engine_process(name){return Ok(());}
    let unknown=||format!("Не удалось определить, какой движок использует {name}. Закройте процесс или повторите проверку.");
    let image=image.ok_or_else(unknown)?;
    let root=process_path_key(root).map_err(|_|unknown())?;
    let image=process_path_key(image).map_err(|_|unknown())?;
    if image.starts_with(&(root+"\\")){return Err(format!("Перед операцией с движком закройте {name}. Готовая загрузка сохранится."));}
    Ok(())
}
fn canonical_guard_root(root:&Path)->Result<PathBuf,String>{
    no_links(root)?;
    let mut existing=root;let mut missing=Vec::new();
    while !existing.exists(){missing.push(existing.file_name().ok_or("Не удалось проверить запущенные приложения")?.to_os_string());existing=existing.parent().ok_or("Не удалось проверить запущенные приложения")?;}
    let mut canonical=existing.canonicalize().map_err(|_|"Не удалось проверить запущенные приложения")?;
    for part in missing.into_iter().rev(){canonical.push(part);}
    Ok(canonical)
}
#[cfg(all(windows, not(test)))]
pub(crate) fn idle(root:&Path) -> Result<(), String> {
    use windows_sys::Win32::{Foundation::{CloseHandle,GetLastError,HANDLE,INVALID_HANDLE_VALUE,ERROR_INVALID_PARAMETER,ERROR_NO_MORE_FILES,WAIT_OBJECT_0,WAIT_TIMEOUT},System::{Diagnostics::ToolHelp::*,Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION,PROCESS_SYNCHRONIZE,QueryFullProcessImageNameW,WaitForSingleObject}}};
    struct Handle(HANDLE);impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
    fn exited(handle:HANDLE)->Result<bool,()>{match unsafe{WaitForSingleObject(handle,0)}{WAIT_OBJECT_0=>Ok(true),WAIT_TIMEOUT=>Ok(false),_=>Err(())}}
    fn process_image(pid:u32)->Result<Option<PathBuf>,()>{
        unsafe{
            let raw=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|PROCESS_SYNCHRONIZE,0,pid);
            if raw.is_null(){return if GetLastError()==ERROR_INVALID_PARAMETER{Ok(None)}else{Err(())};}
            let handle=Handle(raw);if exited(handle.0)?{return Ok(None);}
            let mut buffer=vec![0u16;32768];let mut length=buffer.len() as u32;
            if QueryFullProcessImageNameW(handle.0,0,buffer.as_mut_ptr(),&mut length)==0{return if exited(handle.0)?{Ok(None)}else{Err(())};}
            use std::os::windows::ffi::OsStringExt;
            let image=PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
            let canonical=image.canonicalize();
            if exited(handle.0)?{return Ok(None);}
            canonical.map(Some).map_err(|_|())
        }
    }
    let root=canonical_guard_root(root)?;
    unsafe{
        let raw=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0);
        if raw==INVALID_HANDLE_VALUE{return Err("Не удалось проверить запущенные приложения".into());}
        let snapshot=Handle(raw);let mut entry:PROCESSENTRY32W=std::mem::zeroed();entry.dwSize=std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut available=Process32FirstW(snapshot.0,&mut entry);
        let mut enumeration_error=if available==0{GetLastError()}else{0};
        while available!=0{
            let end=entry.szExeFile.iter().position(|c|*c==0).unwrap_or(entry.szExeFile.len());let name=String::from_utf16_lossy(&entry.szExeFile[..end]);
            if critical_engine_process(&name){
                match process_image(entry.th32ProcessID){
                    Ok(None)=>(),
                    Ok(Some(image))=>{let actual=image.file_name().and_then(|n|n.to_str()).ok_or("Не удалось проверить запущенные приложения")?;guard_engine_process(&root,actual,Some(&image))?;},
                    Err(())=>guard_engine_process(&root,&name,None)?,
                }
            }
            available=Process32NextW(snapshot.0,&mut entry);
            if available==0{enumeration_error=GetLastError();}
        }
        if enumeration_error!=ERROR_NO_MORE_FILES{return Err("Не удалось проверить запущенные приложения".into());}
    }
    Ok(())
}
#[cfg(all(not(windows), not(test)))]
pub(crate) fn idle(_: &Path) -> Result<(), String> {
    Err("Поддерживается Windows".into())
}
fn idle_checkpoint_due(last:Instant)->bool{
    #[cfg(test)]if MOCK_IDLE_PROCESSES.with(|s|s.borrow().is_some()){return true;}
    last.elapsed()>=Duration::from_secs(1)
}
#[tauri::command]
pub fn engine_cancel(state: tauri::State<'_, EngineState>) {
    state.cancel.store(true, Ordering::SeqCst);
    state.paused.store(false, Ordering::SeqCst);
}
#[tauri::command]
pub fn engine_pause(state:tauri::State<'_,EngineState>,paused:bool)->Result<(),String>{
    if !state.active.load(Ordering::SeqCst)||!state.pause_available.load(Ordering::SeqCst){return Err("На этом этапе пауза недоступна".into());}
    state.paused.store(paused,Ordering::SeqCst);Ok(())
}
fn pause_checkpoint(paused:Option<&AtomicBool>,mut report:impl FnMut()){
    while paused.is_some_and(|p|p.load(Ordering::SeqCst)){
        report();std::thread::sleep(Duration::from_millis(250));
    }
}
async fn wait_async(paused:&AtomicBool,cancel:&AtomicBool)->Result<(),String>{
    while paused.load(Ordering::SeqCst){if cancel.load(Ordering::SeqCst){return Err("Загрузка остановлена".into());}tokio::time::sleep(Duration::from_millis(100)).await;}Ok(())
}
fn cancelled(state: &EngineState) -> Result<(), String> {
    if state.cancel.load(Ordering::SeqCst) {
        Err("Загрузка остановлена. Готовые блоки сохранятся для продолжения.".into())
    } else {
        Ok(())
    }
}
async fn chunk(client: reqwest::Client, url: reqwest::Url, cancel: std::sync::Arc<AtomicBool>, paused:std::sync::Arc<AtomicBool>, meter: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Result<Vec<u8>, String> {
    for attempt in 0..3 {
        wait_async(&paused,&cancel).await?;
        if cancel.load(Ordering::SeqCst) { return Err("Проверка остановлена".into()); }
        let response = tokio::time::timeout(Duration::from_secs(90), client.get(url.clone()).send()).await;
        if let Ok(Ok(mut r)) = response {
            if r.status().is_success() {
                let mut bytes = Vec::new();
                let mut failed = false;
                loop {
                    wait_async(&paused,&cancel).await?;
                    match tokio::time::timeout(Duration::from_secs(90), r.chunk()).await {
                        Ok(Ok(Some(b))) => {
                            if bytes.len() + b.len() > 64 * 1024 * 1024 {
                                return Err("Блок слишком велик".into());
                            }
                            crate::download::throttle(b.len(), &cancel).await?;
                            meter.fetch_add(b.len() as u64, Ordering::SeqCst);
                            bytes.extend_from_slice(&b);
                        }
                        Ok(Ok(None)) => break,
                        _ => {
                            failed = true;
                            break;
                        }
                    }
                }
                if !failed {
                    crate::vault::decode_chunk(&bytes)?;
                    return Ok(bytes);
                }
            }
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_secs(1 + attempt)).await;
        }
    }
    Err("Не удалось скачать блок движка. Повторите загрузку.".into())
}
pub(super) fn stage_root(plan: &Plan) -> Result<PathBuf, String> {
    if let Some(stage)=&plan.resume_stage {super::resume::validate_stage(plan,stage)?;return Ok(stage.clone());}
    let root = target(&plan.info.path)?;
    let parent = root.parent().ok_or("Нет папки установки")?;
    let fingerprint =
        digest(format!("{}\n{}", plan.info.path.to_lowercase(), plan.info.build).as_bytes());
    let mut stage = parent.join(format!(".um-engine-{}", &fingerprint[..20]));
    no_links(&stage.join("journal.json"))?;
    if stage.join("journal.json").is_file() {
        let journal: Journal = serde_json::from_slice(&fs::read(stage.join("journal.json")).map_err(io)?)
            .map_err(|_| "Не удалось прочитать журнал восстановления")?;
        if journal.status == "complete" {
            // Keep the previous rollback backup; each subsequent operation gets its own journal.
            stage = parent.join(format!(".um-engine-{}-{}", &fingerprint[..20], plan.info.id));
        }
    }
    no_links(&stage)?;
    Ok(stage)
}
fn operations(plan:&Plan,cancel:&AtomicBool,progress:impl FnMut(verification::Snapshot))->Result<Vec<Operation>,String>{
    let workers=super::scan_storage::workers_hint(&target(&plan.info.path)?);
    verification::operations(plan,cancel,workers,progress)
}
fn verify_current(root: &Path, op: &Operation) -> Result<(), String> {
    let path = root.join(relative(&op.name)?);
    match &op.old_hash {
        Some(h) => {
            if hash_file(&path)? != *h {
                return Err(format!("Файл изменился после проверки: {}", op.name));
            }
        }
        None => {
            no_links(&path)?;
            if path.exists() {
                return Err(format!("В папке появился файл: {}", op.name));
            }
        }
    }
    Ok(())
}
fn rollback(root: &Path, stage: &Path, operations: &[Operation]) -> Result<(), String> {
    rollback_reported(root,stage,operations,|_,_|{})
}
fn rollback_reported(root:&Path,stage:&Path,operations:&[Operation],mut progress:impl FnMut(u64,u64))->Result<(),String>{
    progress(0,operations.len() as u64);
    let mut last=Instant::now();
    for (index,op) in operations.iter().rev().enumerate() {
        let rel = relative(&op.name)?;
        let target = root.join(&rel);
        let original = stage.join("originals").join(&rel);
        no_links(&target)?;
        no_links(&original)?;
        if original.exists() {
            if op
                .old_hash
                .as_ref()
                .is_none_or(|h| hash_file(&original).ok().as_ref() != Some(h))
            {
                return Err(format!("Резервный файл изменён: {}", op.name));
            }
            if target.exists() {
                if op
                    .new_hash
                    .as_ref()
                    .is_none_or(|h| hash_file(&target).ok().as_ref() != Some(h))
                {
                    return Err(format!(
                        "При откате обнаружены внешние изменения: {}",
                        op.name
                    ));
                }
                fs::remove_file(&target).map_err(io)?;
            }
            fs::rename(original, target).map_err(io)?;
        } else if op.old_hash.is_none() && target.exists() {
            if op
                .new_hash
                .as_ref()
                .is_none_or(|h| hash_file(&target).ok().as_ref() != Some(h))
            {
                return Err(format!(
                    "При откате обнаружены внешние изменения: {}",
                    op.name
                ));
            }
            fs::remove_file(target).map_err(io)?;
        }
        if last.elapsed()>=Duration::from_millis(250)||index+1==operations.len(){progress((index+1) as u64,operations.len() as u64);last=Instant::now();}
    }
    Ok(())
}
#[cfg(test)]
fn commit(
    plan: &Plan,
    stage: &Path,
    ops: &[Operation],
    progress: impl FnMut(&str, u64, u64),
) -> Result<(), String> {
    commit_controlled(plan,stage,ops,None,progress)
}
#[cfg(test)]
fn commit_controlled(plan:&Plan,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,progress:impl FnMut(&str,u64,u64))->Result<(),String>{
    commit_checkpointed(plan,stage,ops,paused,None,None,progress).map(|_|())
}
fn commit_checkpointed(plan:&Plan,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,preflight_warning:Option<String>,mut progress:impl FnMut(&str,u64,u64))->Result<Option<String>,String>{
    let root = target(&plan.info.path)?;
    idle(&root)?;
    let journal_path=stage.join("journal.json");no_links(&journal_path)?;
    let previous:Option<Journal>=if journal_path.exists(){Some(serde_json::from_slice(&fs::read(&journal_path).map_err(io)?).map_err(|_|"Повреждён журнал установки")?)}else{None};
    let restoring=previous.as_ref().is_some_and(|j|j.status=="applying");
    let mut journal=if restoring {
        let j=previous.unwrap();if j.root!=plan.info.path||j.build!=plan.info.build{return Err("Журнал относится к другой установке".into());}j
    }else{
        let mut all_ops=ops.to_vec();let item=prepare_metadata(plan,stage,&mut all_ops)?;
        let (launcher,launcher_warning)=match launcher_update(plan){Ok(value)=>(value,None),Err(_)=>(None,Some(REGISTRATION_WARNING.to_string()))};
        Journal{root:plan.info.path.clone(),build:plan.info.build.clone(),operations:all_ops,status:"applying".into(),item,launcher,launcher_warning}
    };
    if preflight_warning.is_some(){journal.launcher_warning=preflight_warning;}
    if let Some((path,_,after))=&journal.launcher{
        if no_links(path).is_ok() && fs::read(path).ok().and_then(|bytes|serde_json::from_slice::<serde_json::Value>(&bytes).ok()).as_ref()==Some(after){journal.launcher_warning=None;}
    }
    let ops=journal.operations.clone();for op in &ops{relative(&op.name)?;}
    fs::create_dir_all(&root).map_err(io)?;
    if !restoring{atomic(&journal_path,&serde_json::to_vec(&journal).map_err(|e|e.to_string())?)?;}
    let marker=root.join(".um-engine-update.json");no_links(&marker)?;
    if marker.exists(){if Path::new(&fs::read_to_string(&marker).map_err(io)?)!=stage{return Err("Есть другая незавершённая установка".into());}}
    else{write_new(&marker,stage.to_string_lossy().as_bytes())?;}
    let mut started=if restoring{ops.clone()}else{Vec::new()};
    let mut created_directories=BTreeSet::new();
    let mut last_progress = Instant::now();
    let mut last_idle = Instant::now();
    let result = (|| -> Result<(), String> {
        if bulk_layout(&root,stage,&ops,paused,suspend,&mut progress)? {
            verify_bulk(&root,stage,&ops,paused,suspend,&mut progress)?;
            pause_checkpoint(paused,||progress("Проверяем файлы перед установкой",ops.len() as u64,ops.len() as u64));
            super::resume::check_suspend(suspend)?;
            idle(&root)?;
            if !empty_installation(&root)? {return Err("В папке появились файлы во время установки".into());}
            
            for entry in fs::read_dir(stage.join("files")).map_err(io)? {
                super::resume::check_suspend(suspend)?;
                let entry=entry.map_err(io)?;let source=entry.path();let destination=root.join(entry.file_name());
                no_links(&source)?;no_links(&destination)?;
                if destination.exists(){return Err("В папке появился файл во время установки".into());}
                fs::rename(source,destination).map_err(io)?;
                let branch=PathBuf::from(entry.file_name());
                if !restoring{for op in &ops{if relative(&op.name)?.starts_with(&branch){started.push(op.clone());}}}
            }
            progress("Устанавливаем движок",ops.len() as u64,ops.len() as u64);
            progress(REGISTRATION_PHASE,0,0);
            update_item(&journal.item,false)?;
            return Ok(());
        }
        for (i, op) in ops.iter().enumerate() {
            pause_checkpoint(paused,||progress("Устанавливаем движок",i as u64,ops.len() as u64));
            super::resume::check_suspend(suspend)?;
            if idle_checkpoint_due(last_idle) {
                idle(&root)?;
                last_idle = Instant::now();
            }
            if restoring && operation_applied(&root,stage,op)? {
                if last_progress.elapsed()>=Duration::from_millis(250)||i+1==ops.len(){progress("Устанавливаем движок",(i+1) as u64,ops.len() as u64);last_progress=Instant::now();}continue;
            }
            let rel = relative(&op.name)?;
            let target = root.join(&rel);
            let source = stage.join("files").join(&rel);
            let backup = stage.join("originals").join(&rel);
            let backup_present=restoring&&backup.exists();
            if !backup_present{verify_current(&root,op)?;}else{if op.old_hash.as_ref()!=Some(&hash_file(&backup)?){return Err("Резервный файл изменён".into());}no_links(&target)?;if target.exists(){return Err("При продолжении обнаружены внешние изменения".into());}}
            if !restoring{started.push(op.clone());}
            if op.old_hash.is_some() && !backup_present {
                no_links(&backup)?;
                fs::create_dir_all(backup.parent().unwrap()).map_err(io)?;
                if backup.exists() {
                    return Err("Осталась предыдущая резервная копия. Нужен откат.".into());
                }
                fs::rename(&target, &backup).map_err(io)?;
            }
            if let Some(hash) = &op.new_hash {
                if hash_file(&source)? != *hash {
                    return Err("Скачанный файл изменился до установки".into());
                }
                let parent=target.parent().unwrap();
                if created_directories.insert(parent.to_path_buf()){fs::create_dir_all(parent).map_err(io)?;}
                fs::rename(source, target).map_err(io)?;
            }
            if last_progress.elapsed() >= Duration::from_millis(250) || i + 1 == ops.len() {
                progress("Устанавливаем движок", (i + 1) as u64, ops.len() as u64);
                last_progress = Instant::now();
            }
        }
        progress(REGISTRATION_PHASE,0,0);
        update_item(&journal.item, false)?;
        Ok(())
    })();
    if let Err(error) = result {
        if error==super::resume::SUSPENDED{return Err(error);}
        match rollback_reported(&root, stage, &started,|done,total|progress(ROLLBACK_PHASE,done,total))
            .and_then(|_| update_item(&journal.item, true))
            .and_then(|_| update_item(&journal.launcher, true))
        {
            Ok(()) => {
                journal.status = "rolledBack".into();
                let _ = atomic(&journal_path, &serde_json::to_vec(&journal).unwrap());
                let _ = fs::remove_file(marker);
                return Err(format!("{error}. Прежние файлы восстановлены."));
            }
            Err(rollback) => {
                return Err(format!(
                    "{error}. Не завершён откат: {rollback}. Резервная копия: {}",
                    stage.display()
                ))
            }
        }
    }
    // Global Epic discovery is optional. A protected LauncherInstalled.dat must
    // never undo verified engine files or the independent Epic .item record.
    if journal.launcher_warning.is_none() && update_item(&journal.launcher,false).is_err(){journal.launcher_warning=Some(REGISTRATION_WARNING.to_string());}
    journal.status = "complete".into();
    atomic(&journal_path, &serde_json::to_vec(&journal).unwrap())?;
    finish_completed_marker(&root,stage,&journal)?;
    Ok(journal.launcher_warning)
}
fn finish_completed_marker(root:&Path,stage:&Path,journal:&Journal)->Result<(),String>{
    let marker=root.join(".um-engine-update.json");no_links(&marker)?;
    if !marker.exists(){return Ok(());}
    if journal.status!="complete" || !super::same_installation_path(&journal.root,&root.to_string_lossy()) || Path::new(&fs::read_to_string(&marker).map_err(io)?)!=stage{
        return Err("Есть другая незавершённая установка".into());
    }
    fs::remove_file(marker).map_err(io)
}
fn operation_applied(root:&Path,stage:&Path,op:&Operation)->Result<bool,String>{
    let rel=relative(&op.name)?;let target=root.join(&rel);let source=stage.join("files").join(&rel);let backup=stage.join("originals").join(&rel);
    no_links(&target)?;no_links(&source)?;no_links(&backup)?;
    if let Some(old)=&op.old_hash{if !backup.exists(){return Ok(false);}if hash_file(&backup)?!=*old{return Err("Резервный файл изменён".into());}
        return match &op.new_hash{None=>if target.exists(){Err("При продолжении обнаружены внешние изменения".into())}else{Ok(true)},Some(new)=>if target.exists(){if hash_file(&target)?==*new{Ok(true)}else{Err("При продолжении обнаружены внешние изменения".into())}}else{Ok(false)}};
    }
    if target.exists(){if op.new_hash.as_ref()==Some(&hash_file(&target)?)&&!source.exists(){Ok(true)}else{Err("При продолжении обнаружены внешние изменения".into())}}else{Ok(false)}
}
fn empty_installation(root:&Path)->Result<bool,String>{
    no_links(root)?;
    for e in fs::read_dir(root).map_err(io)?{if e.map_err(io)?.file_name()!=std::ffi::OsStr::new(".um-engine-update.json"){return Ok(false);}}
    Ok(true)
}
fn bulk_layout(root:&Path,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,progress:&mut impl FnMut(&str,u64,u64))->Result<bool,String>{
    if ops.len()<64||ops.iter().any(|o|o.old_hash.is_some()||o.new_hash.is_none())||!empty_installation(root)?{return Ok(false);}
    let mut last=Instant::now();
    progress("Проверяем файлы перед установкой",0,ops.len() as u64);
    let base=stage.join("files");no_links(&base)?;
    let expected:BTreeSet<_>=ops.iter().map(|o|relative(&o.name)).collect::<Result<_,_>>()?;
    let mut found=BTreeSet::new();let mut pending=vec![base.clone()];
    while let Some(folder)=pending.pop(){
        no_links(&folder)?;
        for e in fs::read_dir(folder).map_err(io)?{
            pause_checkpoint(paused,||progress("Проверяем файлы перед установкой",0,ops.len() as u64));
            super::resume::check_suspend(suspend)?;
            if last.elapsed()>=Duration::from_millis(250){progress("Проверяем файлы перед установкой",0,ops.len() as u64);last=Instant::now();}
            let path=e.map_err(io)?.path();let meta=fs::symlink_metadata(&path).map_err(io)?;
            #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("Ссылки в рабочей папке не поддерживаются".into());}}
            if meta.file_type().is_symlink(){return Err("Ссылки в рабочей папке не поддерживаются".into());}
            if meta.is_dir(){pending.push(path);}else if meta.is_file(){found.insert(path.strip_prefix(&base).map_err(|_|"Путь вне рабочей папки")?.to_path_buf());}else{return Ok(false);}
        }
    }
    Ok(found==expected)
}
fn verify_bulk(root:&Path,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,progress:&mut impl FnMut(&str,u64,u64))->Result<(),String>{
    let mut last=Instant::now();let mut last_idle=Instant::now();
    progress("Проверяем файлы перед установкой",0,ops.len() as u64);
    std::thread::scope(|scope|->Result<(),String>{
        let (send,receive)=std::sync::mpsc::sync_channel(64);
        let failed=std::sync::Arc::new(AtomicBool::new(false));
        let next=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let frontier=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for _ in 0..4 {
            let send=send.clone();let failed=failed.clone();let next=next.clone();let frontier=frontier.clone();
            scope.spawn(move||{loop{
                let i=next.fetch_add(1,Ordering::SeqCst);if i>=ops.len(){break;}
                while (paused.is_some_and(|p|p.load(Ordering::SeqCst))||i>=frontier.load(Ordering::SeqCst)+64)&&!failed.load(Ordering::SeqCst){std::thread::sleep(Duration::from_millis(25));}
                if failed.load(Ordering::SeqCst){break;}
                let op=&ops[i];let result=(||{let source=stage.join("files").join(relative(&op.name)?);if hash_file(&source)?!=*op.new_hash.as_ref().unwrap(){return Err("Скачанный файл изменился до установки".into());}Ok(i)})();
                let bad=result.is_err();if send.send(result).is_err(){break;}if bad{failed.store(true,Ordering::SeqCst);break;}
            }});
        }
        drop(send);let mut seen=BTreeSet::new();let mut contiguous=0usize;
        while contiguous<ops.len(){
            if let Err(e)=super::resume::check_suspend(suspend){failed.store(true,Ordering::SeqCst);return Err(e);}
            match receive.recv_timeout(Duration::from_millis(250)){
                Ok(Ok(i))=>{seen.insert(i);while seen.remove(&contiguous){contiguous+=1;}frontier.store(contiguous,Ordering::SeqCst);},
                Ok(Err(e))=>{failed.store(true,Ordering::SeqCst);return Err(e);},
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>(),
                Err(_)=>{failed.store(true,Ordering::SeqCst);return Err("Проверка установки прервана".into());}
            }
            if idle_checkpoint_due(last_idle){if let Err(e)=idle(root){failed.store(true,Ordering::SeqCst);return Err(e);}last_idle=Instant::now();}
            if last.elapsed()>=Duration::from_millis(250)||contiguous==ops.len(){progress("Проверяем файлы перед установкой",contiguous as u64,ops.len() as u64);last=Instant::now();}
        }
        Ok(())
    })
}
fn prepare_metadata(
    plan: &Plan,
    stage: &Path,
    ops: &mut Vec<Operation>,
) -> Result<Option<(PathBuf, serde_json::Value, serde_json::Value)>, String> {
    let root = target(&plan.info.path)?;
    let name = format!(".egstore/um-{}.manifest", digest(&plan.raw));
    let manifest_path = root.join(relative(&name)?);
    let tags: Vec<_> = plan
        .info
        .components
        .iter()
        .filter(|c| c.selected)
        .map(|c| c.tag.clone())
        .collect();
    let receipt = serde_json::json!({"appName":plan.info.app_name,"catalogItemId":plan.catalog_item_id,"build":plan.info.build,"manifest":manifest_path,"tags":tags,"backup":stage});
    for (name, bytes) in [
        (name, plan.raw.clone()),
        (
            ".egstore/um-install.json".into(),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        ),
    ] {
        let rel = relative(&name)?;
        let dest = root.join(&rel);
        no_links(&dest)?;
        let old_hash = if dest.exists() {
            Some(hash_file(&dest)?)
        } else {
            None
        };
        let new_hash = digest(&bytes);
        if old_hash.as_ref() == Some(&new_hash) {
            continue;
        }
        let source = stage.join("files").join(rel);
        if source.exists() {
            if hash_file(&source)? != new_hash {
                return Err("Несовпадение метаданных в рабочей папке".into());
            }
        } else {
            write_new(&source, &bytes)?;
        }
        ops.push(Operation {
            name,
            old_hash,
            new_hash: Some(new_hash),
        });
    }
    if let Some((path, old)) = &plan.item {
        let mut updated = old.clone();
        updated["AppVersionString"] = plan.info.build.clone().into();
        updated["CompleteManifestPath"] = manifest_path.to_string_lossy().into_owned().into();
        updated["InstallSize"] = plan
            .files
            .values()
            .map(|f| f.size() as u64)
            .sum::<u64>()
            .into();
        updated["bIsIncompleteInstall"] = false.into();
        Ok(Some((path.clone(), old.clone(), updated)))
    } else if let Some(catalog_id)=&plan.catalog_item_id {
        let guid=uuid::Uuid::new_v4().simple().to_string().to_uppercase();
        let path=epic_item_directory().join(format!("{guid}.item"));
        let item=new_epic_item(&plan.info.app_name,catalog_id,&plan.info.build,&root,&manifest_path,plan.files.values().map(|f|f.size() as u64).sum(),&guid);
        Ok(Some((path,serde_json::Value::Null,item)))
    } else { Ok(None) }
}
fn update_item(
    item: &Option<(PathBuf, serde_json::Value, serde_json::Value)>,
    restore: bool,
) -> Result<(), String> {
    if let Some((path, before, after)) = item {
        no_links(path)?;
        let current: serde_json::Value = if path.exists() {
            serde_json::from_slice(&fs::read(path).map_err(io)?)
                .map_err(|_| "Не удалось прочитать запись Epic")?
        } else { serde_json::Value::Null };
        let (expected, target) = if restore {
            (after, before)
        } else {
            (before, after)
        };
        if !restore && current == *after { return Ok(()); }
        if restore && current == *before {
            return Ok(());
        }
        if current != *expected {
            return Err(
                "Epic изменил запись установки. Сохранён журнал для восстановления.".into(),
            );
        }
        if target.is_null() {
            if path.exists() { fs::remove_file(path).map_err(io)?; }
        } else {
            atomic(path, &serde_json::to_vec_pretty(target).unwrap())?;
        }
    }
    Ok(())
}
fn launcher_write_access(path:&Path)->Result<(),String>{
    no_links(path)?;
    if !path.exists(){return Ok(());}
    if fs::metadata(path).map_err(io)?.permissions().readonly(){return Err(REGISTRATION_WARNING.into());}
    #[cfg(windows)]{
        use std::os::windows::fs::OpenOptionsExt;
        // Replacement of an existing file needs DELETE, not merely permission
        // to create a sibling. Opening with this access does not change bytes,
        // attributes or ACLs and never requests elevation.
        fs::OpenOptions::new().access_mode(0x0001_0000).share_mode(0x7).open(path)
            .map_err(|_|REGISTRATION_WARNING.to_string())?;
    }
    Ok(())
}
fn commit_progress(phase:&str,done:u64,total:u64,network:u64,prefix:&[u64],total_bytes:u64,paused:bool)->Progress{
    let registration=phase==REGISTRATION_PHASE;
    let can_pause=!registration && phase!=ROLLBACK_PHASE;
    let (done,total)=if registration{(0,0)}else{(done,total)};
    Progress{phase:phase.into(),done,total,percent:done.saturating_mul(100)/total.max(1),network_bytes:network,
        processed_bytes:if can_pause{prefix.get(done as usize).copied().unwrap_or(total_bytes)}else{0},total_bytes:if can_pause{total_bytes}else{0},
        files_done:done,files_total:total,paused:can_pause&&paused,can_pause,..Default::default()}
}
fn validation_progress(phase:&str,done:u64,total:u64,bytes:u64,total_bytes:u64,paused:bool)->Progress{
    Progress{phase:phase.into(),done,total,percent:done.saturating_mul(100)/total.max(1),network_bytes:0,
        processed_bytes:bytes,total_bytes,files_done:done,files_total:total,paused,can_pause:true,..Default::default()}
}
fn validation_checkpoint(cancel:&AtomicBool,paused:&AtomicBool,suspend:&AtomicBool,mut report:impl FnMut())->Result<(),String>{
    loop{
        super::resume::check_suspend(Some(suspend))?;
        if cancel.load(Ordering::SeqCst){return Err("Загрузка остановлена. Готовые блоки сохранятся для продолжения.".into());}
        if !paused.load(Ordering::SeqCst){return Ok(());}
        report();std::thread::sleep(Duration::from_millis(250));
    }
}
fn validate_cached_chunks(stage:&Path,chunks:BTreeMap<String,reqwest::Url>,cancel:&AtomicBool,paused:&AtomicBool,suspend:&AtomicBool,mut progress:impl FnMut(Progress))->Result<(u64,Vec<(String,reqwest::Url)>),String>{
    let total=chunks.len() as u64;let mut cached=Vec::new();let mut pending=Vec::new();let mut total_bytes=0u64;
    progress(validation_progress(CACHE_INVENTORY_PHASE,0,total,0,0,paused.load(Ordering::SeqCst)));
    let mut last=Instant::now();
    for (index,(guid,url)) in chunks.into_iter().enumerate(){
        validation_checkpoint(cancel,paused,suspend,||progress(validation_progress(CACHE_INVENTORY_PHASE,index as u64,total,0,0,true)))?;
        if !guid.chars().all(|c|c.is_ascii_hexdigit()||c=='-')||guid.is_empty(){return Err("Некорректный идентификатор блока".into());}
        let path=stage.join("chunks").join(&guid);no_links(&path)?;
        if path.exists(){let size=fs::metadata(&path).map_err(io)?.len();total_bytes=total_bytes.saturating_add(size);cached.push((path,size));}
        else{pending.push((guid,url));}
        if last.elapsed()>=Duration::from_millis(250)||index as u64+1==total{progress(validation_progress(CACHE_INVENTORY_PHASE,index as u64+1,total,0,0,paused.load(Ordering::SeqCst)));last=Instant::now();}
    }
    let total=cached.len() as u64;let mut done=0u64;let mut bytes=0u64;
    progress(validation_progress(CACHE_VALIDATION_PHASE,0,total,0,total_bytes,paused.load(Ordering::SeqCst)));
    last=Instant::now();
    for (path,size) in cached{
        validation_checkpoint(cancel,paused,suspend,||progress(validation_progress(CACHE_VALIDATION_PHASE,done,total,bytes,total_bytes,true)))?;
        no_links(&path)?;let stored=fs::read(path).map_err(io)?;
        if stored.len() as u64!=size{return Err("Скачанный файл изменился до установки".into());}
        crate::vault::decode_chunk(&stored)?;
        done+=1;bytes=bytes.saturating_add(stored.len() as u64);
        if last.elapsed()>=Duration::from_millis(250)||done==total{progress(validation_progress(CACHE_VALIDATION_PHASE,done,total,bytes,total_bytes,paused.load(Ordering::SeqCst)));last=Instant::now();}
    }
    validation_checkpoint(cancel,paused,suspend,||progress(validation_progress(CACHE_VALIDATION_PHASE,done,total,bytes,total_bytes,true)))?;
    Ok((done,pending))
}
#[tauri::command]
pub async fn engine_install(
    app: tauri::AppHandle,
    epic: tauri::State<'_,crate::epic::EpicState>,
    state: tauri::State<'_, EngineState>,
    vault: tauri::State<'_, crate::vault::VaultState>,
    id: String,
    on_progress: tauri::ipc::Channel<Progress>,
) -> Result<String, String> {
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true, Ordering::SeqCst) {
        return Err("Уже выполняется операция с движком".into());
    }
    let _busy = Busy(&state.active);
    if vault.active.swap(true, Ordering::SeqCst) {
        return Err("Дождитесь текущей операции с хранилищем".into());
    }
    let _vault = Busy(&vault.active);
    state.cancel.store(false, Ordering::SeqCst);
    state.paused.store(false,Ordering::SeqCst);
    state.pause_available.store(false,Ordering::SeqCst);
    state.suspend.store(false,Ordering::SeqCst);state.resumable.store(false,Ordering::SeqCst);
    let _pause_scope=PauseScope(&state);
    let mut plan = state
        .plan
        .lock()
        .map_err(|_| "Ошибка состояния")?
        .as_ref()
        .filter(|p| p.info.id == id)
        .cloned()
        .ok_or("Подготовьте установку заново")?;
    if !plan.info.conflicts.is_empty() {
        return Err("Есть локальные изменения. Установка не начата.".into());
    }
    let _installation_lock = lock_installation(&target(&plan.info.path)?)?;
    let stage = stage_root(&plan)?;
    plan.resume_stage=Some(stage.clone());
    let _=on_progress.send(Progress{phase:"Сохраняем задачу установки".into(),done:0,total:0,percent:0,network_bytes:0,processed_bytes:0,total_bytes:0,files_done:0,files_total:0,paused:false,can_pause:false,..Default::default()});
    plan=tauri::async_runtime::spawn_blocking(move||{super::resume::save(&plan)?;Ok::<_,String>(plan)}).await.map_err(|_|"Сохранение установки прервано")??;
    state.resumable.store(true,Ordering::SeqCst);
    // Probe the optional global record before download without changing it.
    let launcher_preflight_warning=launcher_write_access(&launcher_path()).err().map(|_|REGISTRATION_WARNING.to_string());
    if stage.join("journal.json").exists() {
        let j: Journal = serde_json::from_slice(&fs::read(stage.join("journal.json")).map_err(io)?)
            .map_err(|_| "Не удалось прочитать журнал восстановления")?;
        if j.status=="applying" {let (saved_stage,_)=recovery(&target(&plan.info.path)?)?;if saved_stage!=stage{return Err("Не совпадает папка восстановления".into());}}
        if !matches!(j.status.as_str(),"rolledBack"|"applying"|"complete") {
            return Err(format!(
                "Найдена предыдущая операция ({}). Журнал: {}",
                j.status,
                stage.display()
            ));
        }
    }
    let progress_task_id=plan.info.id.clone();
    let emit = |phase: &str, done: u64, total: u64, network_bytes: u64| {
        super::resume::report(&progress_task_id,phase,done,total);
        let _ = on_progress.send(Progress {
            phase: phase.into(),
            done,
            total,
            percent: if total == 0 {
                0
            } else {
                done.saturating_mul(100) / total
            },
            network_bytes,
            processed_bytes: if phase=="Собираем и проверяем файлы" {done}else{0},
            total_bytes: if phase=="Собираем и проверяем файлы" {total}else{0},
            files_done: if phase==TASK_VALIDATION_PHASE{done}else{0}, files_total: if phase==TASK_VALIDATION_PHASE{total}else{0},
            paused:state.paused.load(Ordering::SeqCst),can_pause:state.pause_available.load(Ordering::SeqCst),..Default::default()
        });
    };
    emit(TASK_VALIDATION_PHASE, 0, 0, 0);
    let saved_ops=super::resume::load_operations(&stage)?.map(|ops|ops.into_iter().filter(|o|!o.name.starts_with(".egstore/")).collect::<Vec<_>>());
    let ops = if let Some(ops)=saved_ops {ops} else {
        emit("Проверяем установленные файлы",0,0,0);
        let cancel = state.cancel.clone();
        let channel = on_progress.clone();
        let (returned_plan,result)=tauri::async_runtime::spawn_blocking(move || {
        let result=operations(&plan, &cancel, |snapshot| {
            let _ = channel.send(Progress {
                phase: "Проверяем установленные файлы".into(),
                done:snapshot.done,
                total:snapshot.total,
                percent:snapshot.done.saturating_mul(100)/snapshot.total.max(1),
                network_bytes: 0,
                processed_bytes:snapshot.processed_bytes,total_bytes:0,files_done:snapshot.done,files_total:snapshot.total,paused:false,can_pause:false,
                current_file:snapshot.current_file,files_per_second:Some(snapshot.files_per_second),read_bytes_per_second:Some(snapshot.read_bytes_per_second),
            });
        });
        (plan,result)
    })
    .await
    .map_err(|_| "Проверка прервана")?;plan=returned_plan;result?};
    super::resume::save_operations(&stage,&ops)?;
    cancelled(&state)?;
    let bytes: u64 = ops
        .iter()
        .filter_map(|o| plan.files.get(&o.name))
        .map(|f| f.size() as u64)
        .sum();
    let mut chunks = BTreeMap::new();
    let already_committed=super::resume::commit_complete(&stage)?;
    let applying=super::resume::commit_applying(&stage)?;
    let mut remaining_bytes=0u64;
    let mut task_validation_last=Instant::now();
    emit(TASK_VALIDATION_PHASE,0,ops.len() as u64,0);
    for (index,op) in ops.iter().enumerate() {
        super::resume::check_suspend(Some(&state.suspend))?;cancelled(&state)?;
        if op.new_hash.is_some() && !already_committed && !applying {
            let file=plan.files.get(&op.name).ok_or("Повреждена сохранённая задача установки")?;
            if op.new_hash.as_ref()!=Some(&file.file_hash){return Err("Файлы задачи не совпадают с манифестом".into());}
            if !stage.join("files").join(&op.name).exists(){remaining_bytes=remaining_bytes.saturating_add(file.size() as u64);}
            for part in &file.file_chunk_parts {chunks.entry(part.guid.clone()).or_insert(part.link.clone().ok_or("Нет адреса блока")?);}
        }
        if task_validation_last.elapsed()>=Duration::from_millis(250)||index+1==ops.len(){
            let mut p=validation_progress(TASK_VALIDATION_PHASE,index as u64+1,ops.len() as u64,0,0,false);p.can_pause=false;
            super::resume::report(&plan.info.id,TASK_VALIDATION_PHASE,p.done,p.total);let _=on_progress.send(p);task_validation_last=Instant::now();
        }
    }
    // Reserve the assembled files and a conservative uncompressed chunk cache.
    let reserve = remaining_bytes
        .saturating_add(
            chunks
                .keys()
                .filter(|g|!stage.join("chunks").join(g).exists())
                .map(|g| plan.chunk_sizes.get(g).copied().unwrap_or(64 * 1024 * 1024))
                .sum::<u64>(),
        )
        .saturating_add(if already_committed||applying{0}else{1024 * 1024 * 1024});
    if free(&target(&plan.info.path)?)? < reserve {
        return Err(format!(
            "Недостаточно места: требуется до {:.1} ГБ",
            reserve as f64 / 1073741824.
        ));
    }
    fs::create_dir_all(stage.join("chunks")).map_err(io)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Не удалось создать загрузчик")?;
    let total = chunks.len() as u64;
    let meter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    state.pause_available.store(true,Ordering::SeqCst);
    let validation_stage=stage.clone();let validation_cancel=state.cancel.clone();let validation_pause=state.paused.clone();let validation_suspend=state.suspend.clone();
    let validation_channel=on_progress.clone();let validation_id=plan.info.id.clone();
    let (mut done,pending)=tauri::async_runtime::spawn_blocking(move||validate_cached_chunks(&validation_stage,chunks,&validation_cancel,&validation_pause,&validation_suspend,|p|{
        super::resume::report(&validation_id,&p.phase,p.done,p.total);let _=validation_channel.send(p);
    })).await.map_err(|_|"Проверка прервана")??;
    emit("Скачиваем движок", done, total, meter.load(Ordering::SeqCst));
    for batch in pending.chunks(8) {
        while state.paused.load(Ordering::SeqCst){cancelled(&state)?;emit("Скачиваем движок",done,total,meter.load(Ordering::SeqCst));tokio::time::sleep(Duration::from_millis(250)).await;}
        cancelled(&state)?;
        let mut tasks = tokio::task::JoinSet::new();
        for (guid, url) in batch {
            let c = client.clone();
            let url = url.clone();
            let guid = guid.clone();
            let cancel = state.cancel.clone();
            let counter = meter.clone();
            let paused=state.paused.clone();
            tasks.spawn(async move { Ok::<_, String>((guid, chunk(c, url, cancel, paused, counter).await?)) });
        }
        let mut heartbeat = tokio::time::interval(Duration::from_millis(500));
        while !tasks.is_empty() {
            let result = tokio::select! {
                result = tasks.join_next() => match result {Some(result) => result, None => break},
                _ = heartbeat.tick() => {cancelled(&state)?; emit("Скачиваем движок", done, total, meter.load(Ordering::SeqCst)); continue;}
            };
            let (guid, raw) = result.map_err(|_| "Загрузка прервана")??;
            cancelled(&state)?;
            atomic(&stage.join("chunks").join(guid), &raw)?;
            done += 1;
            emit("Скачиваем движок", done, total, meter.load(Ordering::SeqCst));
        }
    }
    let network = meter.load(Ordering::SeqCst);
    if !already_committed && !applying {
    let assembly_id=plan.info.id.clone();
    let assembly_plan=plan.clone();let assembly_stage=stage.clone();let assembly_ops=ops.clone();let assembly_cancel=state.cancel.clone();let assembly_channel=on_progress.clone();let assembly_pause=state.paused.clone();
    tauri::async_runtime::spawn_blocking(move||assemble_files_controlled(&assembly_plan,&assembly_stage,&assembly_ops,&assembly_cancel,Some(&assembly_pause),|done,files_done,files_total|{
        super::resume::report(&assembly_id,"Собираем и проверяем файлы",done,bytes);
        let _=assembly_channel.send(Progress{phase:"Собираем и проверяем файлы".into(),done,total:bytes,percent:done*100/bytes.max(1),network_bytes:network,processed_bytes:done,total_bytes:bytes,files_done,files_total,paused:assembly_pause.load(Ordering::SeqCst),can_pause:true,..Default::default()});
    })).await.map_err(|_|"Сборка прервана")??;}
    cancelled(&state)?;
    emit("Устанавливаем движок", 0, ops.len() as u64, network);
    let copy = plan.clone();
    let stage_copy = stage.clone();
    let channel = on_progress.clone();
    let mut prefix=vec![0u64];
    for op in &ops {prefix.push(prefix.last().copied().unwrap_or(0).saturating_add(plan.files.get(&op.name).or_else(||plan.old.get(&op.name)).map(|f|f.size() as u64).unwrap_or(0)));}
    let total_bytes=prefix.last().copied().unwrap_or(0);
    let commit_id=plan.info.id.clone();
    let commit_pause=state.paused.clone();let commit_suspend=state.suspend.clone();
    let commit_app=app.clone();
    let registration_warning=if !already_committed {tauri::async_runtime::spawn_blocking(move || {
        commit_checkpointed(&copy, &stage_copy, &ops,Some(&commit_pause),Some(&commit_suspend),launcher_preflight_warning, |phase, done, total| {
            if phase==REGISTRATION_PHASE||phase==ROLLBACK_PHASE {commit_app.state::<EngineState>().pause_available.store(false,Ordering::SeqCst);commit_pause.store(false,Ordering::SeqCst);}
            super::resume::report(&commit_id,phase,done,total);
            let _=channel.send(commit_progress(phase,done,total,network,&prefix,total_bytes,commit_pause.load(Ordering::SeqCst)));
        })
    })
    .await
    .map_err(|_| "Установка прервана. Проверьте журнал восстановления")??}else{
        let journal:Journal=serde_json::from_slice(&fs::read(stage.join("journal.json")).map_err(io)?).map_err(|_|"Повреждён журнал установки")?;
        finish_completed_marker(&target(&plan.info.path)?,&stage,&journal)?;
        journal.launcher_warning
    };
    super::resume::check_suspend(Some(&state.suspend))?;
    state.pause_available.store(false,Ordering::SeqCst);state.paused.store(false,Ordering::SeqCst);
    let installed_root = target(&plan.info.path)?;
    register_root(&installed_root)?;
    let branch=plan.info.app_name.trim_start_matches("UE_");
    let fab_needed=!installed_root.join("Engine/Plugins/Fab/Fab.uplugin").is_file()&&!installed_root.join("Engine/Plugins/Marketplace/Fab/Fab.uplugin").is_file()
        && branch.split('.').filter_map(|n|n.parse::<u32>().ok()).collect::<Vec<_>>().as_slice()>=[5,3].as_slice();
    let mut fab_warning=None;
    let fab_received=std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    if fab_needed {
        emit("Устанавливаем Fab",0,0,network);
        let channel=on_progress.clone();
        let fab_meter=fab_received.clone();
        let fab_channel=tauri::ipc::Channel::<crate::vault::Progress>::new(move|body|{
            if let tauri::ipc::InvokeResponseBody::Json(json)=body {
                if let Ok(p)=serde_json::from_str::<serde_json::Value>(&json) {
                    let _=channel.send(fab_progress(&p,network,&fab_meter));
                }
            }
            Ok(())
        });
        if let Err(error)=crate::vault::install_fab_locked(&app,&epic,&vault,&installed_root,branch,fab_channel).await {fab_warning=Some(error);}
    }
    let network=network.saturating_add(fab_received.load(Ordering::SeqCst));
    super::resume::check_suspend(Some(&state.suspend))?;
    emit("Устанавливаем системные компоненты",0,0,network);
    let prerequisite_plan=plan.clone();
    let prerequisite_channel=on_progress.clone();let prerequisite_suspend=state.suspend.clone();
    let prerequisites=tauri::async_runtime::spawn_blocking(move||super::prerequisites::install(&prerequisite_plan,Some(&prerequisite_suspend),|phase,done,total|{
        let waiting=phase=="Ожидаем завершения установки системных компонентов";
        let _=prerequisite_channel.send(Progress{phase:phase.into(),done:if waiting{0}else{done},total:if waiting{0}else{total},percent:if waiting{0}else{done.saturating_mul(100)/total.max(1)},network_bytes:network,processed_bytes:0,total_bytes:0,files_done:done,files_total:total,paused:false,can_pause:false,..Default::default()});
    })).await.map_err(|_|"Проверка системных компонентов прервана")?;
    super::resume::check_suspend(Some(&state.suspend))?;
    emit("Очищаем временные файлы", 0, 0, network);
    let cleanup_root=installed_root.clone();
    let cleanup_stage=stage.clone();
    let cleanup_suspend=state.suspend.clone();let cleanup_channel=on_progress.clone();
    let cleanup=tauri::async_runtime::spawn_blocking(move || cleanup_completed_controlled(&cleanup_root,&cleanup_stage,crate::storage::keep_engine_backup(),Some(&cleanup_suspend),|phase,done,total,bytes,total_bytes|{
        let mut p=removal_progress(phase,done,total,bytes,total_bytes);p.network_bytes=network;let _=cleanup_channel.send(p);
    })).await.map_err(|_|"Очистка прервана")?;
    super::resume::check_suspend(Some(&state.suspend))?;
    super::resume::remove(&plan.info.id)?;
    state.resumable.store(false,Ordering::SeqCst);
    *state.plan.lock().map_err(|_| "Ошибка состояния")? = None;
    emit("Движок установлен", 1, 1, network);
    let mut result=match cleanup {
        Ok(())=>format!("{} установлена. Временные файлы очищены.",plan.info.build),
        Err(error)=>format!("{} установлена. Не удалось очистить временные файлы: {error}. Папка: {}",plan.info.build,stage.display()),
    };
    match prerequisites {Ok(true)=>result.push_str(" Требуется перезагрузка Windows."),Ok(false)=>(),Err(error)=>{result.push_str(" Движок установлен, но системные компоненты требуют внимания: ");result.push_str(&error);}}
    if let Some(error)=fab_warning {result.push_str(" Fab не установлен: ");result.push_str(&error);}
    if registration_warning.is_some(){result.push('\n');result.push_str(REGISTRATION_WARNING);}
    Ok(result)
}

#[derive(Clone,Copy,PartialEq)]
enum RemovalKind{File,Directory,FileLink,DirectoryLink}
struct RemovalEntry{path:PathBuf,kind:RemovalKind,bytes:u64}
fn removal_metadata(path:&Path)->Result<(RemovalKind,u64),String>{
    let meta=fs::symlink_metadata(path).map_err(io)?;
    #[cfg(windows)]let (link,directory)={use std::os::windows::fs::MetadataExt;((meta.file_attributes()&0x400)!=0,(meta.file_attributes()&0x10)!=0)};
    #[cfg(not(windows))]let (link,directory)=(meta.file_type().is_symlink(),meta.is_dir());
    if link{return Ok((if directory{RemovalKind::DirectoryLink}else{RemovalKind::FileLink},0));}
    Ok(if directory{(RemovalKind::Directory,0)}else{(RemovalKind::File,meta.len())})
}
pub(super) fn removal_progress(phase:&str,done:u64,total:u64,bytes:u64,total_bytes:u64)->Progress{
    let mut p=validation_progress(phase,done,total,bytes,total_bytes,false);p.can_pause=false;p
}
pub(super) fn remove_trees_counted(roots:&[PathBuf],inventory_phase:&str,removal_phase:&str,suspend:Option<&AtomicBool>,mut progress:impl FnMut(&str,u64,u64,u64,u64))->Result<(),String>{
    let mut entries=Vec::new();let mut pending=Vec::new();let mut total_bytes=0u64;let mut last=Instant::now();
    progress(inventory_phase,0,0,0,0);
    for root in roots{no_links(root)?;if root.exists(){pending.push(root.clone());}}
    while let Some(path)=pending.pop(){
        super::resume::check_suspend(suspend)?;
        no_links(path.parent().ok_or("Путь файла")?)?;
        let (kind,bytes)=removal_metadata(&path)?;
        if kind==RemovalKind::Directory{for child in fs::read_dir(&path).map_err(io)?{pending.push(child.map_err(io)?.path());}}
        total_bytes=total_bytes.saturating_add(bytes);entries.push(RemovalEntry{path,kind,bytes});
        if last.elapsed()>=Duration::from_millis(250){progress(inventory_phase,entries.len() as u64,0,0,0);last=Instant::now();}
    }
    progress(inventory_phase,entries.len() as u64,0,0,0);
    let total=entries.len() as u64;let mut done=0u64;let mut bytes=0u64;last=Instant::now();
    progress(removal_phase,0,total,0,total_bytes);
    for entry in entries.into_iter().rev(){
        super::resume::check_suspend(suspend)?;
        no_links(entry.path.parent().ok_or("Путь файла")?)?;
        let current=removal_metadata(&entry.path)?;
        if current!=(entry.kind,entry.bytes){return Err("Файлы изменились во время удаления. Повторите проверку.".into());}
        match entry.kind{RemovalKind::Directory|RemovalKind::DirectoryLink=>fs::remove_dir(&entry.path),RemovalKind::File|RemovalKind::FileLink=>fs::remove_file(&entry.path)}.map_err(io)?;
        done+=1;bytes=bytes.saturating_add(entry.bytes);
        if last.elapsed()>=Duration::from_millis(250)||done==total{progress(removal_phase,done,total,bytes,total_bytes);last=Instant::now();}
    }
    Ok(())
}
#[cfg(test)]
fn cleanup_completed(root:&Path,stage:&Path,keep_backup:bool)->Result<(),String>{cleanup_completed_controlled(root,stage,keep_backup,None,|_,_,_,_,_|{})}
fn cleanup_completed_controlled(root:&Path,stage:&Path,keep_backup:bool,suspend:Option<&AtomicBool>,mut progress:impl FnMut(&str,u64,u64,u64,u64))->Result<(),String> {
    no_links(root)?; no_links(stage)?;
    let mut journal:Journal=serde_json::from_slice(&fs::read(stage.join("journal.json")).map_err(io)?).map_err(|_|"Повреждён журнал установки")?;
    let fingerprint=digest(format!("{}\n{}",journal.root.to_lowercase(),journal.build).as_bytes());
    let prefix=format!(".um-engine-{}",&fingerprint[..20]);
    if journal.status!="complete" || !super::same_installation_path(&journal.root,&root.to_string_lossy()) || root.join(".um-engine-update.json").exists()
        || stage.parent()!=root.parent() || !stage.file_name().and_then(|n|n.to_str()).is_some_and(|n|n==prefix || n.strip_prefix(&(prefix.clone()+"-")).is_some_and(|s|uuid::Uuid::parse_str(s).is_ok())) {
        return Err("Очистка разрешена только для завершённой установки и её рабочей папки.".into());
    }
    let mut cleanup_roots=Vec::new();
    for name in ["chunks","files","originals"] {
        if name=="originals" && keep_backup {continue;}
        let folder=stage.join(name);
        no_links(&folder)?;
        if folder.exists() {cleanup_roots.push(folder);}
    }
    remove_trees_counted(&cleanup_roots,"Считаем временные файлы","Удаляем временные файлы",suspend,&mut progress)?;
    super::resume::check_suspend(suspend)?;
    progress("Завершаем очистку",0,0,0,0);
    let saved_ops=stage.join("resume-operations.zlib");no_links(&saved_ops)?;if saved_ops.exists(){fs::remove_file(saved_ops).map_err(io)?;}
    let originals=stage.join("originals");
    let has_backup=originals.is_dir() && fs::read_dir(&originals).map_err(io)?.next().is_some();
    if !keep_backup || !has_backup {
        journal.operations.clear();
        journal.item=None; journal.launcher=None;
        atomic(&stage.join("journal.json"),&serde_json::to_vec(&journal).map_err(|e|e.to_string())?)?;
        let receipt=root.join(".egstore/um-install.json");
        if receipt.exists() {
            no_links(&receipt)?;
            let mut value:serde_json::Value=serde_json::from_slice(&fs::read(&receipt).map_err(io)?).map_err(|e|e.to_string())?;
            value["backup"]=serde_json::Value::Null;
            atomic(&receipt,&serde_json::to_vec_pretty(&value).map_err(|e|e.to_string())?)?;
        }
        // Only remove the known journal and an empty directory, never unknown files.
        fs::remove_file(stage.join("journal.json")).map_err(io)?;
        if originals.exists() {fs::remove_dir(&originals).map_err(io)?;}
        fs::remove_dir(stage).map_err(io)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_restores_original_and_removes_new() {
        let root = std::env::temp_dir().join(format!("um-engine-test-{}", uuid::Uuid::new_v4()));
        let stage = root.join("stage");
        let dest = root.join("engine");
        fs::create_dir_all(&dest).unwrap();
        write_new(&stage.join("originals/a"), b"old").unwrap();
        write_new(&dest.join("a"), b"new").unwrap();
        write_new(&dest.join("b"), b"added").unwrap();
        let ops = vec![
            Operation {
                name: "a".into(),
                old_hash: Some(digest(b"old")),
                new_hash: Some(digest(b"new")),
            },
            Operation {
                name: "b".into(),
                old_hash: None,
                new_hash: Some(digest(b"added")),
            },
        ];
        rollback(&dest, &stage, &ops).unwrap();
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"old");
        assert!(!dest.join("b").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rollback_preserves_external_edit() {
        let root = std::env::temp_dir().join(format!("um-engine-test-{}", uuid::Uuid::new_v4()));
        let stage = root.join("stage");
        let dest = root.join("engine");
        write_new(&stage.join("originals/a"), b"old").unwrap();
        write_new(&dest.join("a"), b"external").unwrap();
        assert!(rollback(
            &dest,
            &stage,
            &[Operation {
                name: "a".into(),
                old_hash: Some(digest(b"old")),
                new_hash: Some(digest(b"new"))
            }]
        )
        .is_err());
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"external");
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[derive(Clone)]
struct MockEngineProcess{name:String,image:Option<PathBuf>}
#[cfg(test)]
thread_local!{static MOCK_IDLE_PROCESSES:std::cell::RefCell<Option<std::collections::VecDeque<Vec<MockEngineProcess>>>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(crate) fn idle(root:&Path) -> Result<(), String> {
    let processes=MOCK_IDLE_PROCESSES.with(|slot|{let mut slot=slot.borrow_mut();slot.as_mut().map(|snapshots|if snapshots.len()>1{snapshots.pop_front().unwrap()}else{snapshots.front().cloned().unwrap_or_default()})});
    let Some(processes)=processes else{return Ok(());};
    let root=canonical_guard_root(root)?;
    for process in processes{guard_engine_process(&root,&process.name,process.image.as_deref())?;}
    Ok(())
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    struct MockIdleScope(Option<std::collections::VecDeque<Vec<MockEngineProcess>>>);
    impl MockIdleScope{fn new(snapshots:Vec<Vec<MockEngineProcess>>)->Self{Self(MOCK_IDLE_PROCESSES.with(|s|s.replace(Some(snapshots.into()))))}}
    impl Drop for MockIdleScope{fn drop(&mut self){MOCK_IDLE_PROCESSES.with(|s|{s.replace(self.0.take());});}}
    #[test]
    fn engine_process_guard_scopes_case_extended_unc_and_sibling_paths(){
        let root=Path::new(r"X:\Fixture\UE_5.7");
        assert!(guard_engine_process(root,"UnrealEditor-Cmd.exe",Some(Path::new(r"\\?\x:\fixture\ue_5.7\Engine\Binaries\Win64\UnrealEditor-Cmd.exe"))).is_err());
        assert!(guard_engine_process(root,"ShaderCompileWorker.exe",Some(Path::new("X:/Fixture/UE_5.7/Engine/Binaries/Win64/ShaderCompileWorker.exe"))).is_err());
        assert!(guard_engine_process(root,"UnrealBuildTool.exe",Some(Path::new(r"X:\Fixture\UE_5.8\Engine\Binaries\UnrealBuildTool.exe"))).is_ok());
        assert!(guard_engine_process(root,"UnrealEditor.exe",Some(Path::new(r"X:\Fixture\UE_5.7_other\UnrealEditor.exe"))).is_ok());
        assert!(guard_engine_process(root,"dotnet.exe",Some(Path::new(r"X:\Fixture\UE_5.7\Engine\Binaries\ThirdParty\DotNet\dotnet.exe"))).is_err());
        assert!(guard_engine_process(root,"EpicGamesLauncher.exe",None).is_ok());
        assert!(guard_engine_process(Path::new(r"\\server\share\UE"),"UnrealEditor.exe",Some(Path::new(r"\\?\UNC\SERVER\share\UE\Engine\UnrealEditor.exe"))).is_err());
        assert!(guard_engine_process(Path::new(r"\\server\share\UE"),"UnrealEditor.exe",Some(Path::new(r"\\server\other\UE\UnrealEditor.exe"))).is_ok());
        let error=guard_engine_process(root,"UnrealEditor.exe",None).unwrap_err();assert!(error.contains("Не удалось определить, какой движок"));
        assert!(guard_engine_process(root,"UnrealEditor.exe",Some(Path::new("UnrealEditor.exe"))).unwrap_err().contains("Не удалось определить"));
    }
    #[cfg(windows)]
    #[test]
    fn canonical_process_image_alias_cannot_bypass_target_guard(){
        let base=std::env::temp_dir().join(format!("um-process-alias-{}",uuid::Uuid::new_v4()));let root=base.join("engine");let alias=base.join("alias");
        fs::create_dir_all(root.join("Engine/Binaries")).unwrap();fs::write(root.join("Engine/Binaries/UnrealEditor.exe"),b"fixture").unwrap();
        let status=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:UM_TEST_LINK -Target $env:UM_TEST_TARGET | Out-Null"]).env("UM_TEST_LINK",&alias).env("UM_TEST_TARGET",&root).status().unwrap();assert!(status.success());
        let image=alias.join("Engine/Binaries/UnrealEditor.exe").canonicalize().unwrap();let canonical=canonical_guard_root(&root).unwrap();
        assert!(guard_engine_process(&canonical,"UnrealEditor.exe",Some(&image)).is_err());
        assert_eq!(canonical_guard_root(&base.join("missing/target")).unwrap(),base.canonicalize().unwrap().join("missing/target"));
        fs::remove_dir(alias).unwrap();fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn midcommit_unrelated_editor_is_allowed_but_target_editor_rolls_back(){
        for own in [false,true]{
            let (base,stage,plan,mut ops)=fixture();let root=PathBuf::from(&plan.info.path);let b="Engine/new-file";
            write_new(&stage.join("files").join(b),b"new file").unwrap();ops.push(Operation{name:b.into(),old_hash:None,new_hash:Some(digest(b"new file"))});
            let process_root=if own{root.clone()}else{base.join("unrelated-engine")};let process=MockEngineProcess{name:"UnrealEditor-Cmd.exe".into(),image:Some(process_root.join("Engine/Binaries/Win64/UnrealEditor-Cmd.exe"))};
            // Initial guard and the first file pass; the process appears before
            // the next file. Test-only checkpoint scheduling avoids wall time.
            let _scope=MockIdleScope::new(vec![vec![],vec![],vec![process]]);
            let mut phases=vec![];let result=commit_checkpointed(&plan,&stage,&ops,None,None,None,|phase,_,_|phases.push(phase.to_string()));
            if own{assert!(result.unwrap_err().contains("закройте UnrealEditor-Cmd.exe"));assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"old");assert!(!root.join(b).exists());assert!(phases.iter().any(|p|p==ROLLBACK_PHASE));}
            else{result.unwrap();assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"new");assert_eq!(fs::read(root.join(b)).unwrap(),b"new file");assert!(!phases.iter().any(|p|p==ROLLBACK_PHASE));}
            assert!(!root.join(".um-engine-update.json").exists());fs::remove_dir_all(base).unwrap();
        }
    }
    #[test]
    fn engine_install_future_has_bounded_inline_state(){
        fn return_size<F>(_:fn(tauri::AppHandle,tauri::State<'static,crate::epic::EpicState>,tauri::State<'static,EngineState>,tauri::State<'static,crate::vault::VaultState>,String,tauri::ipc::Channel<Progress>)->F)->usize{std::mem::size_of::<F>()}
        let bytes=return_size(engine_install);
        println!("engine_install future inline bytes: {bytes}");
        assert!(bytes<=64*1024,"engine command future must not embed large I/O buffers; inline bytes: {bytes}");
    }
    #[cfg(windows)]
    #[test]
    fn protected_launcher_record_does_not_roll_back_engine_or_epic_item(){
        use std::os::windows::fs::OpenOptionsExt;
        for fresh in [false,true]{
            let (base,stage,plan,ops)=if fresh{fresh_fixture()}else{fixture()};
            let root=PathBuf::from(&plan.info.path);let launcher=base.join("LauncherInstalled.dat");
            let before=br#"{"InstallationList":[{"AppName":"Game","InstallLocation":"X:/Fixture/Game"}]}"#;
            fs::write(&launcher,before).unwrap();
            assert!(launcher_write_access(&launcher).is_ok());
            let metadata=launcher_update_at(&launcher,&plan.info.app_name,&plan.info.build,&root).unwrap();
            let mut all_ops=ops.clone();let item=prepare_metadata(&plan,&stage,&mut all_ops).unwrap();
            let journal=Journal{root:plan.info.path.clone(),build:plan.info.build.clone(),operations:all_ops,status:"applying".into(),item:item.clone(),launcher:Some(metadata),launcher_warning:None};
            atomic(&stage.join("journal.json"),&serde_json::to_vec(&journal).unwrap()).unwrap();
            // Simulate permissions/sharing changing after a successful preflight:
            // reading and sibling creation work, replacement is denied.
            let held=fs::OpenOptions::new().read(true).share_mode(0x3).open(&launcher).unwrap();
            assert!(launcher_write_access(&launcher).is_err());
            let mut phases=vec![];
            let preflight=if fresh{launcher_write_access(&launcher).err().map(|_|REGISTRATION_WARNING.to_string())}else{None};
            let warning=commit_checkpointed(&plan,&stage,&ops,None,None,preflight,|phase,_,_|phases.push(phase.to_string())).unwrap();
            assert_eq!(warning.as_deref(),Some(REGISTRATION_WARNING));
            assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"new");
            assert_eq!(fs::read(&launcher).unwrap(),before);
            if let Some((path,_,after))=item{assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap(),after);}
            let completed:Journal=serde_json::from_slice(&fs::read(stage.join("journal.json")).unwrap()).unwrap();
            assert_eq!(completed.status,"complete");assert_eq!(completed.launcher_warning.as_deref(),Some(REGISTRATION_WARNING));
            assert!(!root.join(".um-engine-update.json").exists());assert!(!phases.iter().any(|p|p==ROLLBACK_PHASE));
            assert!(phases.iter().any(|p|p==REGISTRATION_PHASE));
            assert!(!fs::read_dir(&base).unwrap().flatten().any(|e|e.path().extension().is_some_and(|s|s.to_string_lossy().starts_with("um-"))));
            drop(held);fs::remove_dir_all(base).unwrap();
        }
    }
    #[test]
    fn registration_is_exactly_idempotent_and_never_overwrites_foreign_changes(){
        let (base,_,plan,_)=fixture();let path=base.join("idempotent.item");
        let before=serde_json::json!({"version":"before"});let after=serde_json::json!({"version":"after"});
        let item=Some((path.clone(),before,after.clone()));
        fs::write(&path,after.to_string()).unwrap();let modified=fs::metadata(&path).unwrap().modified().unwrap();
        update_item(&item,false).unwrap();assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(),modified);
        fs::write(&path,br#"{"version":"foreign-change"}"#).unwrap();
        assert!(update_item(&item,false).is_err());assert_eq!(fs::read(&path).unwrap(),br#"{"version":"foreign-change"}"#);
        let _=plan;fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn completed_journal_finalizes_only_its_own_marker_after_restart(){
        let (base,stage,plan,ops)=fixture();let root=PathBuf::from(&plan.info.path);
        commit(&plan,&stage,&ops,|_,_,_|{}).unwrap();
        let journal:Journal=serde_json::from_slice(&fs::read(stage.join("journal.json")).unwrap()).unwrap();
        fs::write(root.join(".um-engine-update.json"),stage.to_string_lossy().as_bytes()).unwrap();
        finish_completed_marker(&root,&stage,&journal).unwrap();finish_completed_marker(&root,&stage,&journal).unwrap();
        assert!(!root.join(".um-engine-update.json").exists());
        fs::write(root.join(".um-engine-update.json"),base.join("unrelated-stage").to_string_lossy().as_bytes()).unwrap();
        assert!(finish_completed_marker(&root,&stage,&journal).is_err());
        assert!(root.join(".um-engine-update.json").exists());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn metadata_and_rollback_progress_cannot_show_stale_installation_bytes_or_pause(){
        let progress=commit_progress(REGISTRATION_PHASE,999,999,50,&[0,500],500,true);
        assert_eq!((progress.done,progress.total,progress.percent,progress.files_done,progress.files_total,progress.processed_bytes,progress.total_bytes),(0,0,0,0,0,0,0));
        assert!(!progress.can_pause);assert!(!progress.paused);
        let progress=commit_progress(ROLLBACK_PHASE,3,6,50,&[0,500],500,true);
        assert_eq!((progress.done,progress.total,progress.percent,progress.files_done,progress.files_total),(3,6,50,3,6));
        assert_eq!((progress.processed_bytes,progress.total_bytes),(0,0));assert!(!progress.can_pause);assert!(!progress.paused);
    }
    #[test]
    fn real_rollback_reports_its_own_phase_and_restores_original_file(){
        let (base,stage,plan,mut ops)=fixture();let root=PathBuf::from(&plan.info.path);
        write_new(&stage.join("files/Engine/b"),b"corrupt").unwrap();
        ops.push(Operation{name:"Engine/b".into(),old_hash:None,new_hash:Some(digest(b"expected"))});
        let mut phases=vec![];
        assert!(commit_checkpointed(&plan,&stage,&ops,None,None,None,|phase,done,total|phases.push((phase.to_string(),done,total))).is_err());
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"old");
        assert!(phases.iter().any(|(phase,done,total)|phase==ROLLBACK_PHASE&&*done==0&&*total>0));
        assert!(phases.iter().any(|(phase,done,total)|phase==ROLLBACK_PHASE&&done==total));
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn registration_creates_launcher_record_preserves_games_and_rolls_back(){
        let (base,_,plan,_)=fixture();let path=base.join("LauncherInstalled.dat");let root=Path::new(&plan.info.path);
        let item=launcher_update_at(&path,"UE_5.8","5.8.3-3",root).unwrap();assert!(item.1.is_null());update_item(&Some(item.clone()),false).unwrap();assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()["InstallationList"][0]["AppName"],"UE_5.8");update_item(&Some(item),true).unwrap();assert!(!path.exists());
        fs::write(&path,br#"{"InstallationList":[{"AppName":"Game","InstallLocation":"X:/Fixture/Game"}],"other":1}"#).unwrap();let item=launcher_update_at(&path,"UE_5.8","5.8.3-3",root).unwrap();assert_eq!(item.2["InstallationList"][0]["AppName"],"Game");assert_eq!(item.2["other"],1);assert_eq!(item.2["InstallationList"].as_array().unwrap().len(),2);
        let other=base.join("other-engine");fs::create_dir(&other).unwrap();fs::write(&path,serde_json::json!({"InstallationList":[{"AppName":"UE_5.8","InstallLocation":other}]}).to_string()).unwrap();assert!(launcher_update_at(&path,"UE_5.8","5.8.3-3",root).is_err());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn new_epic_item_matches_engine_and_manifest(){
        let (base,_,plan,_)=fixture();let root=Path::new(&plan.info.path);let manifest=root.join(".egstore/um-test.manifest");let item=new_epic_item("UE_5.8","catalog-fixture","5.8.3-3",root,&manifest,123,"guid-fixture");
        assert_eq!(item["AppName"],"UE_5.8");assert_eq!(item["CatalogNamespace"],"ue");assert_eq!(item["CatalogItemId"],"catalog-fixture");assert_eq!(item["InstallSize"],123);assert_eq!(item["CompleteManifestPath"],manifest.to_string_lossy().as_ref());assert_eq!(item["bIsIncompleteInstall"],false);fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn cleanup_only_complete_owned_stage_and_optional_backup(){
        let (base,old_stage,plan,ops)=fixture();let root=Path::new(&plan.info.path);let stage=stage_root(&plan).unwrap();fs::rename(old_stage,&stage).unwrap();commit(&plan,&stage,&ops,|_,_,_|{}).unwrap();fs::create_dir_all(stage.join("chunks")).unwrap();fs::write(stage.join("chunks/cache"),b"download").unwrap();
        fs::write(root.join(".um-engine-update.json"),b"pending").unwrap();assert!(cleanup_completed(root,&stage,false).is_err());assert!(stage.join("chunks/cache").exists());fs::remove_file(root.join(".um-engine-update.json")).unwrap();
        let suspend=AtomicBool::new(false);let mut interrupted=vec![];
        assert_eq!(cleanup_completed_controlled(root,&stage,true,Some(&suspend),|phase,done,total,bytes,total_bytes|{
            interrupted.push((phase.to_string(),done,total,bytes,total_bytes));if phase=="Удаляем временные файлы"{suspend.store(true,Ordering::SeqCst);}
        }).unwrap_err(),super::super::resume::SUSPENDED);
        assert!(stage.join("chunks/cache").is_file());assert!(stage.join("journal.json").is_file());assert!(stage.join("originals/Engine/a").is_file());
        assert!(interrupted.iter().any(|e|e.0=="Считаем временные файлы"&&e.1>=2&&e.2==0));
        assert!(interrupted.iter().all(|e|e.0!="Завершаем очистку"));
        suspend.store(false,Ordering::SeqCst);let mut events=vec![];
        cleanup_completed_controlled(root,&stage,true,Some(&suspend),|phase,done,total,bytes,total_bytes|events.push((phase.to_string(),done,total,bytes,total_bytes))).unwrap();
        let removed=events.iter().filter(|e|e.0=="Удаляем временные файлы").last().unwrap();assert_eq!(removed.1,removed.2);assert_eq!(removed.3,removed.4);assert!(removed.3>=8);
        assert_eq!(events.last().unwrap(),&("Завершаем очистку".into(),0,0,0,0));
        assert!(!stage.join("chunks").exists());assert!(stage.join("originals/Engine/a").is_file());assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"new");
        cleanup_completed(root,&stage,false).unwrap();assert!(!stage.join("originals").exists());assert_eq!(fs::read(root.join("Engine/Plugins/UserPlugin/x")).unwrap(),b"user plugin");
        let receipt:serde_json::Value=serde_json::from_slice(&fs::read(root.join(".egstore/um-install.json")).unwrap()).unwrap();assert!(receipt["backup"].is_null());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn counted_removal_rejects_changed_entry_before_deleting_it(){
        let root=std::env::temp_dir().join(format!("um-removal-test-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();fs::write(root.join("a"),b"old").unwrap();
        let mut events=vec![];
        let error=remove_trees_counted(&[root.clone()],"inventory","remove",None,|phase,done,total,bytes,total_bytes|{
            events.push((phase.to_string(),done,total,bytes,total_bytes));if phase=="remove"&&done==0{fs::write(root.join("a"),b"changed after inventory").unwrap();}
        }).unwrap_err();
        assert_eq!(error,"Файлы изменились во время удаления. Повторите проверку.");assert_eq!(fs::read(root.join("a")).unwrap(),b"changed after inventory");
        assert!(events.iter().all(|e|e.0!="remove"||e.1==0));fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn fab_child_local_work_retains_phase_counts_and_received_network(){
        let received=std::sync::atomic::AtomicU64::new(0);
        let download=fab_progress(&serde_json::json!({"phase":"Скачиваем файлы","done":4,"total":8,"networkBytes":40,"filesDone":1,"filesTotal":2}),100,&received);
        assert_eq!(download.phase,"Fab: Скачиваем файлы");assert_eq!((download.done,download.total,download.network_bytes),(4,8,140));
        let copy=fab_progress(&serde_json::json!({"phase":"Копируем и проверяем файлы","done":12,"total":20,"networkBytes":0,"filesDone":1,"filesTotal":2}),100,&received);
        assert_eq!(copy.phase,"Fab: Копируем и проверяем файлы");assert_eq!((copy.percent,copy.processed_bytes,copy.total_bytes,copy.files_done,copy.files_total,copy.network_bytes),(60,12,20,1,2,140));assert!(!copy.can_pause);
        let next=fab_progress(&serde_json::json!({"phase":"Подготавливаем файлы","done":0,"total":0,"networkBytes":60}),100,&received);
        assert_eq!(next.network_bytes,160);assert_eq!(next.total,0);
    }
    fn test_chunk(data:&[u8])->Vec<u8>{let mut bytes=vec![0u8;41];bytes[0..4].copy_from_slice(&2986228386u32.to_le_bytes());bytes[4..8].copy_from_slice(&1u32.to_le_bytes());bytes[8..12].copy_from_slice(&41u32.to_le_bytes());bytes[12..16].copy_from_slice(&(data.len() as u32).to_le_bytes());bytes.extend_from_slice(data);bytes}
    #[test]
    fn block_cache_reuses_interleaved_blocks_and_evicts_within_bound(){
        let (base,stage,_,_)=fixture();fs::create_dir_all(stage.join("chunks")).unwrap();for (n,data) in [("aa",b"AAAA"),("bb",b"BBBB"),("cc",b"CCCC")]{fs::write(stage.join("chunks").join(n),test_chunk(data)).unwrap();}
        let inputs=||["aa","bb","cc","dd"].into_iter().map(|name|(name.to_string(),format!("https://example.invalid/{name}").parse().unwrap())).collect();
        let cancel=AtomicBool::new(false);let paused=AtomicBool::new(false);let suspend=AtomicBool::new(false);
        let mut reports=vec![];let mut saw_pause=false;
        let (valid,pending)=validate_cached_chunks(&stage,inputs(),&cancel,&paused,&suspend,|p|{
            if p.phase==CACHE_VALIDATION_PHASE&&p.done==0{if p.paused{saw_pause=true;paused.store(false,Ordering::SeqCst);}else if !saw_pause{paused.store(true,Ordering::SeqCst);}}
            reports.push(p);
        }).unwrap();
        assert_eq!(valid,3);assert_eq!(pending.len(),1);assert_eq!(pending[0].0,"dd");assert!(saw_pause);
        let checked:Vec<_>=reports.iter().filter(|p|p.phase==CACHE_VALIDATION_PHASE).collect();
        assert_eq!((checked[0].done,checked[0].total,checked[0].processed_bytes),(0,3,0));
        let last=checked.last().unwrap();assert_eq!((last.done,last.total,last.percent,last.processed_bytes,last.total_bytes,last.files_done,last.files_total),(3,3,100,135,135,3,3));
        assert!(checked.windows(2).all(|p|p[0].done<=p[1].done&&p[0].processed_bytes<=p[1].processed_bytes));
        assert!(reports.iter().all(|p|p.network_bytes==0));
        assert!(validate_cached_chunks(&stage,inputs(),&cancel,&paused,&suspend,|p|{if p.phase==CACHE_VALIDATION_PHASE{cancel.store(true,Ordering::SeqCst);}}).is_err());
        cancel.store(false,Ordering::SeqCst);
        assert_eq!(validate_cached_chunks(&stage,inputs(),&cancel,&paused,&suspend,|p|{if p.phase==CACHE_VALIDATION_PHASE{suspend.store(true,Ordering::SeqCst);}}).unwrap_err(),super::super::resume::SUSPENDED);
        suspend.store(false,Ordering::SeqCst);assert_eq!(fs::read(stage.join("chunks/aa")).unwrap(),test_chunk(b"AAAA"));
        let mut cache=BlockCache::new(8);assert_eq!(cache.get(&stage,"aa").unwrap(),b"AAAA");assert_eq!(cache.get(&stage,"bb").unwrap(),b"BBBB");fs::remove_file(stage.join("chunks/aa")).unwrap();assert_eq!(cache.get(&stage,"aa").unwrap(),b"AAAA");cache.get(&stage,"cc").unwrap();assert!(cache.bytes<=8);assert!(!cache.blocks.contains_key("bb"));assert!(cache.get(&stage,"../x").is_err());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn assembly_checks_hashes_resumes_and_reports_bytes_and_files(){
        use egs_api::api::types::download_manifest::FileChunkPart;
        let (base,stage,mut plan,_)=fixture();fs::create_dir_all(stage.join("chunks")).unwrap();fs::write(stage.join("chunks/aa"),test_chunk(b"assembled")).unwrap();let file=FileManifestList{filename:"Engine/b".into(),file_hash:digest(b"assembled"),file_chunk_parts:vec![FileChunkPart{guid:"aa".into(),size:9,..Default::default()}]};plan.files.insert(file.filename.clone(),file.clone());let ops=vec![Operation{name:file.filename.clone(),old_hash:None,new_hash:Some(file.file_hash)}];let mut reports=vec![];assemble_files(&plan,&stage,&ops,&AtomicBool::new(false),|a,b,c|reports.push((a,b,c))).unwrap();assert_eq!(reports.last(),Some(&(9,1,1)));assert_eq!(fs::read(stage.join("files/Engine/b")).unwrap(),b"assembled");fs::remove_file(stage.join("chunks/aa")).unwrap();assemble_files(&plan,&stage,&ops,&AtomicBool::new(false),|_,_,_|{}).unwrap();fs::write(stage.join("files/Engine/b"),b"bad").unwrap();assert!(assemble_files(&plan,&stage,&ops,&AtomicBool::new(false),|_,_,_|{}).is_err());assert!(assemble_files(&plan,&stage,&ops,&AtomicBool::new(true),|_,_,_|{}).is_err());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn installation_progress_is_bounded_for_many_small_files(){
        let (base,stage,mut plan,mut ops)=fixture();plan.item=None;
        for i in 0..1000 {let name=format!("Engine/tiny/{i}.txt");write_new(&stage.join("files").join(&name),b"tiny").unwrap();ops.push(Operation{name,old_hash:None,new_hash:Some(digest(b"tiny"))});}
        let start=Instant::now();let mut count=0;let mut last=(0,0);commit(&plan,&stage,&ops,|_,done,total|{count+=1;last=(done,total);}).unwrap();assert_eq!(last.0,last.1);assert!(count<=start.elapsed().as_millis() as usize/250+2);println!("1000 files installed in {:?}; {} progress messages",start.elapsed(),count);fs::remove_dir_all(base).unwrap();
    }

    fn fresh_fixture() -> (PathBuf, PathBuf, Plan, Vec<Operation>) {
        let (base, stage, mut plan, mut ops) = fixture();
        let root = PathBuf::from(&plan.info.path);
        assert_eq!(root, base.join("engine"));
        fs::remove_dir_all(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        plan.item = None;
        plan.info.update = false;
        ops[0].old_hash = None;
        for i in 0..1000 {
            let name = format!("Engine/tiny/{i}.txt");
            write_new(&stage.join("files").join(&name), b"tiny").unwrap();
            ops.push(Operation { name, old_hash: None, new_hash: Some(digest(b"tiny")) });
        }
        (base, stage, plan, ops)
    }
    #[test]
    fn restart_continues_update_without_recopying_applied_files() {
        let (base,stage,plan,ops)=fixture();let root=PathBuf::from(&plan.info.path);
        let stop=AtomicBool::new(true);
        assert_eq!(commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).unwrap_err(),super::super::resume::SUSPENDED);
        write_new(&stage.join("originals/Engine/a"),b"old").unwrap();
        fs::remove_file(root.join("Engine/a")).unwrap();
        fs::rename(stage.join("files/Engine/a"),root.join("Engine/a")).unwrap();
        let modified=fs::metadata(root.join("Engine/a")).unwrap().modified().unwrap();
        stop.store(false,Ordering::SeqCst);
        commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).unwrap();
        assert_eq!(fs::metadata(root.join("Engine/a")).unwrap().modified().unwrap(),modified);
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"new");
        assert_eq!(fs::read(stage.join("originals/Engine/a")).unwrap(),b"old");
        assert!(!root.join(".um-engine-update.json").exists());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn restart_recovers_between_backup_and_replacement() {
        let (base,stage,plan,ops)=fixture();let root=PathBuf::from(&plan.info.path);let stop=AtomicBool::new(true);
        assert!(commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).is_err());
        fs::create_dir_all(stage.join("originals/Engine")).unwrap();
        fs::rename(root.join("Engine/a"),stage.join("originals/Engine/a")).unwrap();
        stop.store(false,Ordering::SeqCst);commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).unwrap();
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"new");fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn restart_after_bulk_directory_move_preserves_completed_branch() {
        let (base,stage,plan,ops)=fresh_fixture();let root=PathBuf::from(&plan.info.path);let stop=AtomicBool::new(true);
        assert!(commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).is_err());
        fs::rename(stage.join("files/Engine"),root.join("Engine")).unwrap();
        let modified=fs::metadata(root.join("Engine/a")).unwrap().modified().unwrap();
        stop.store(false,Ordering::SeqCst);commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).unwrap();
        assert_eq!(fs::metadata(root.join("Engine/a")).unwrap().modified().unwrap(),modified);
        assert!(root.join(".egstore/um-install.json").exists());fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn restart_never_overwrites_an_edited_applied_file() {
        let (base,stage,plan,ops)=fixture();let root=PathBuf::from(&plan.info.path);let stop=AtomicBool::new(true);
        assert!(commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).is_err());
        write_new(&stage.join("originals/Engine/a"),b"old").unwrap();fs::write(root.join("Engine/a"),b"user edit").unwrap();
        stop.store(false,Ordering::SeqCst);assert!(commit_checkpointed(&plan,&stage,&ops,None,Some(&stop),None,|_,_,_|{}).is_err());
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(),b"user edit");assert_eq!(fs::read(stage.join("originals/Engine/a")).unwrap(),b"old");fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn saved_task_round_trips_exact_plan_and_stage_after_restart() {
        let (base,old_stage,mut plan,ops)=fixture();plan.info.id=uuid::Uuid::new_v4().to_string();let stage=stage_root(&plan).unwrap();fs::rename(old_stage,&stage).unwrap();plan.resume_stage=Some(stage.clone());
        super::super::resume::save_at(&base.join("tasks"),&plan).unwrap();super::super::resume::save_operations(&stage,&ops).unwrap();
        let restored=super::super::resume::load_at(&base.join("tasks"),&plan.info.id).unwrap();
        assert_eq!(restored.info.build,plan.info.build);assert_eq!(restored.raw,plan.raw);assert_eq!(stage_root(&restored).unwrap(),stage);assert_eq!(super::super::resume::load_operations(&stage).unwrap().unwrap()[0].name,"Engine/a");
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn fresh_install_verifies_then_moves_directories() {
        let (base, stage, plan, ops) = fresh_fixture();
        let mut phases = Vec::new();
        let start = Instant::now();
        commit(&plan, &stage, &ops, |phase, _, _| phases.push(phase.to_owned())).unwrap();
        println!("Fresh installation of 1001 files: {:?}", start.elapsed());
        let root = PathBuf::from(&plan.info.path);
        assert!(phases.iter().any(|p| p == "Проверяем файлы перед установкой"));
        assert_eq!(fs::read(root.join("Engine/tiny/999.txt")).unwrap(), b"tiny");
        assert!(root.join(".egstore/um-install.json").exists());
        assert!(!stage.join("files/Engine").exists());
        assert!(!root.join(".um-engine-update.json").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn fresh_install_rejects_corruption_before_moving_any_directory() {
        let (base, stage, plan, ops) = fresh_fixture();
        fs::write(stage.join("files/Engine/tiny/0.txt"), b"corrupt").unwrap();
        assert!(commit(&plan, &stage, &ops, |_, _, _| {}).is_err());
        let root = PathBuf::from(&plan.info.path);
        assert!(!root.join("Engine").exists());
        assert!(stage.join("files/Engine/a").exists());
        assert!(!root.join(".um-engine-update.json").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn extra_staged_file_uses_serial_path_and_is_not_installed() {
        let (base, stage, plan, ops) = fresh_fixture();
        fs::write(stage.join("files/Engine/extra.txt"), b"extra").unwrap();
        commit(&plan, &stage, &ops, |_, _, _| {}).unwrap();
        assert!(stage.join("files/Engine").exists());
        assert!(!PathBuf::from(&plan.info.path).join("Engine/extra.txt").exists());
        assert!(stage.join("files/Engine/extra.txt").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn installation_pause_retains_files_until_resumed() {
        let (base, stage, plan, ops) = fixture();
        let root = PathBuf::from(&plan.info.path);
        let paused = std::sync::Arc::new(AtomicBool::new(true));
        let worker_paused = paused.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || commit_controlled(&plan, &stage, &ops, Some(&worker_paused), |_, _, _| { let _ = send.send(()); }));
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(), b"old");
        paused.store(false, Ordering::SeqCst);
        thread.join().unwrap().unwrap();
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(), b"new");
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn bulk_metadata_failure_rolls_back_moved_directories() {
        let (base, stage, mut plan, ops) = fresh_fixture();
        let bad_item = base.join("missing/engine.item");
        plan.item = Some((bad_item, serde_json::json!({"AppName":"UE_5.8"})));
        assert!(commit(&plan, &stage, &ops, |_, _, _| {}).is_err());
        let root = PathBuf::from(&plan.info.path);
        assert!(!root.join("Engine/a").exists());
        assert!(!root.join(".egstore/um-install.json").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn existing_user_files_disable_bulk_install_and_are_preserved() {
        let (base, stage, plan, ops) = fresh_fixture();
        let root = PathBuf::from(&plan.info.path);
        write_new(&root.join("Engine/Plugins/UserPlugin/keep"), b"user").unwrap();
        let mut bulk = false;
        commit(&plan, &stage, &ops, |phase, _, _| bulk |= phase == "Проверяем файлы перед установкой").unwrap();
        assert!(!bulk);
        assert_eq!(fs::read(root.join("Engine/Plugins/UserPlugin/keep")).unwrap(), b"user");
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn bulk_verification_pause_keeps_destination_empty() {
        let (base, stage, plan, ops) = fresh_fixture();
        let root = PathBuf::from(&plan.info.path);
        let paused = std::sync::Arc::new(AtomicBool::new(true));
        let worker_paused = paused.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || commit_controlled(&plan, &stage, &ops, Some(&worker_paused), |_, done, _| { let _ = send.send(done); }));
        assert_eq!(receive.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
        assert_eq!(receive.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
        assert!(!root.join("Engine").exists());
        paused.store(false, Ordering::SeqCst);
        thread.join().unwrap().unwrap();
        assert!(root.join("Engine/a").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[tokio::test]
    async fn paused_download_can_be_cancelled() {
        let paused = AtomicBool::new(true);
        let cancel = AtomicBool::new(true);
        assert!(tokio::time::timeout(Duration::from_secs(1), wait_async(&paused, &cancel)).await.unwrap().is_err());
    }
    #[test]
    fn paused_assembly_resumes_and_can_be_cancelled() {
        use egs_api::api::types::download_manifest::FileChunkPart;
        let (base, stage, mut plan, _) = fixture();
        fs::create_dir_all(stage.join("chunks")).unwrap();
        fs::write(stage.join("chunks/aa"), test_chunk(b"assembled")).unwrap();
        let file = FileManifestList { filename: "Engine/b".into(), file_hash: digest(b"assembled"), file_chunk_parts: vec![FileChunkPart { guid: "aa".into(), size: 9, ..Default::default() }] };
        plan.files.insert(file.filename.clone(), file.clone());
        let ops = vec![Operation { name: file.filename, old_hash: None, new_hash: Some(file.file_hash) }];
        let paused = AtomicBool::new(true);
        let cancel = AtomicBool::new(false);
        let mut waiting = 0;
        assemble_files_controlled(&plan, &stage, &ops, &cancel, Some(&paused), |done, _, _| {
            if paused.load(Ordering::SeqCst) { assert_eq!(done, 0); assert!(!stage.join("files/Engine/b").exists()); waiting += 1; paused.store(false, Ordering::SeqCst); }
        }).unwrap();
        assert_eq!(waiting, 1);
        assert_eq!(fs::read(stage.join("files/Engine/b")).unwrap(), b"assembled");
        fs::remove_file(stage.join("files/Engine/b")).unwrap();
        paused.store(true, Ordering::SeqCst);
        assert!(assemble_files_controlled(&plan, &stage, &ops, &cancel, Some(&paused), |_, _, _| { cancel.store(true, Ordering::SeqCst); paused.store(false, Ordering::SeqCst); }).is_err());
        assert!(!stage.join("files/Engine/b").exists());
        fs::remove_dir_all(base).unwrap();
    }

    fn fixture() -> (PathBuf, PathBuf, Plan, Vec<Operation>) {
        let base =
            std::env::temp_dir().join(format!("um-engine-transaction-{}", uuid::Uuid::new_v4()));
        let root = base.join("engine");
        let stage = base.join("stage");
        write_new(&root.join("Engine/a"), b"old").unwrap();
        write_new(&root.join("Engine/Plugins/UserPlugin/x"), b"user plugin").unwrap();
        write_new(&stage.join("files/Engine/a"), b"new").unwrap();
        let info = Preview {
            id: "test".into(),
            app_name: "UE_5.8".into(),
            build: "5.8.3-3".into(),
            path: root.to_string_lossy().into_owned(),
            update: true,
            components: vec![],
            bytes: 3,
            download_bytes: 3,
            changed: 1,
            added: 0,
            removed: 0,
            required_bytes: 3,
            free_bytes: 100,
            conflicts: vec![],
        };
        let item_path = base.join("engine.item");
        let item = serde_json::json!({"AppName":"UE_5.8","AppVersionString":"5.8.1-1","InstallLocation":root});
        write_new(&item_path, &serde_json::to_vec(&item).unwrap()).unwrap();
        let plan = Plan {
            resume_stage: None,
            catalog_item_id: None,
            remove_unselected: false,
            source_base: String::new(),
            info,
            raw: b"test manifest".to_vec(),
            files: BTreeMap::new(),
            old: BTreeMap::new(),
            item: Some((item_path, item)),
            chunk_sizes: BTreeMap::new(),
            tags: BTreeMap::new(),
        };
        (
            base,
            stage,
            plan,
            vec![Operation {
                name: "Engine/a".into(),
                old_hash: Some(digest(b"old")),
                new_hash: Some(digest(b"new")),
            }],
        )
    }
    #[test]
    fn component_changes_add_missing_remove_deselected_and_preserve_user_files() {
        let (base, _, mut plan, _) = fixture();
        let root = PathBuf::from(&plan.info.path);
        let existing = FileManifestList { filename: "Engine/a".into(), file_hash: digest(b"old"), ..Default::default() };
        let missing = FileManifestList { filename: "Engine/debug.pdb".into(), file_hash: digest(b"symbols"), ..Default::default() };
        plan.old.insert(existing.filename.clone(), existing.clone());
        plan.old.insert(missing.filename.clone(), missing.clone());
        plan.tags.insert(existing.filename.clone(), vec!["source".into()]);
        plan.tags.insert(missing.filename.clone(), vec!["symbols".into()]);
        plan.files.insert(missing.filename.clone(), missing);
        plan.remove_unselected = true;
        let ops = operations(&plan, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().any(|o| o.name == "Engine/debug.pdb" && o.old_hash.is_none() && o.new_hash.is_some()));
        assert!(ops.iter().any(|o| o.name == "Engine/a" && o.new_hash.is_none()));
        assert!(root.join("Engine/Plugins/UserPlugin/x").exists());
        fs::write(root.join("Engine/a"), b"user edited").unwrap();
        assert!(operations(&plan, &AtomicBool::new(false), |_| {}).is_err());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn modify_requires_exact_build() {
        assert!(same_build("5.8.3-58210709+++UE5", "5.8.3", "58210709"));
        assert!(!same_build("5.8.3-58210709+++UE5", "5.8.1", "58210709"));
        assert!(!same_build("5.8.3-58210709+++UE5", "5.8.3", "1"));
    }
    #[test]
    fn commits_files_metadata_and_keeps_plugins() {
        let (base, stage, plan, ops) = fixture();
        commit(&plan, &stage, &ops, |_, _, _| {}).unwrap();
        let root = PathBuf::from(&plan.info.path);
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(), b"new");
        assert_eq!(fs::read(stage.join("originals/Engine/a")).unwrap(), b"old");
        assert_eq!(
            fs::read(root.join("Engine/Plugins/UserPlugin/x")).unwrap(),
            b"user plugin"
        );
        let v: serde_json::Value =
            serde_json::from_slice(&fs::read(&plan.item.as_ref().unwrap().0).unwrap()).unwrap();
        assert_eq!(v["AppVersionString"], "5.8.3-3");
        assert!(!root.join(".um-engine-update.json").exists());
        let canonical_stage = stage_root(&plan).unwrap();
        fs::create_dir_all(&canonical_stage).unwrap();
        fs::copy(stage.join("journal.json"), canonical_stage.join("journal.json")).unwrap();
        let next_stage = stage_root(&plan).unwrap();
        assert_ne!(canonical_stage, next_stage);
        assert!(canonical_stage.join("journal.json").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn mid_commit_failure_restores_originals_and_metadata() {
        let (base, stage, plan, mut ops) = fixture();
        ops.push(Operation {
            name: "Engine/missing".into(),
            old_hash: None,
            new_hash: Some(digest(b"missing")),
        });
        assert!(commit(&plan, &stage, &ops, |_, _, _| {}).is_err());
        let root = PathBuf::from(&plan.info.path);
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(), b"old");
        let v: serde_json::Value =
            serde_json::from_slice(&fs::read(&plan.item.as_ref().unwrap().0).unwrap()).unwrap();
        assert_eq!(v["AppVersionString"], "5.8.1-1");
        assert!(!root.join(".um-engine-update.json").exists());
        assert!(!root.join(".egstore/um-install.json").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn concurrent_edit_is_never_overwritten() {
        let (base, stage, plan, ops) = fixture();
        let root = PathBuf::from(&plan.info.path);
        fs::write(root.join("Engine/a"), b"external").unwrap();
        assert!(commit(&plan, &stage, &ops, |_, _, _| {}).is_err());
        assert_eq!(fs::read(root.join("Engine/a")).unwrap(), b"external");
        fs::remove_dir_all(base).unwrap();
    }
}

#[derive(Serialize)]
pub struct Pending {
    path: String,
    build: String,
}
fn recovery(root: &Path) -> Result<(PathBuf, Journal), String> {
    no_links(root)?;
    let marker = root.join(".um-engine-update.json");
    no_links(&marker)?;
    let stage = PathBuf::from(fs::read_to_string(marker).map_err(io)?);
    if stage.parent() != root.parent()
        || !stage
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with(".um-engine-"))
    {
        return Err("Некорректная папка восстановления".into());
    }
    no_links(&stage)?;
    let path = stage.join("journal.json");
    no_links(&path)?;
    let j: Journal = serde_json::from_slice(&fs::read(path).map_err(io)?)
        .map_err(|_| "Повреждён журнал восстановления")?;
    if Path::new(&j.root) != root {
        return Err("Журнал относится к другой установке".into());
    }
    for op in &j.operations {
        relative(&op.name)?;
    }
    if let Some((path, before, after)) = &j.item {
        let created=before.is_null();
        let valid_path=if created {path.parent()==Some(epic_item_directory().as_path()) && path.file_stem().and_then(|s|s.to_str()).is_some_and(|s|uuid::Uuid::parse_str(s).is_ok()) && path.extension().and_then(|s|s.to_str())==Some("item")}else{items().iter().any(|(p,_)|p==path)};
        if !valid_path
            || after["InstallLocation"] != root.to_string_lossy().as_ref()
            || !created && before["InstallLocation"] != after["InstallLocation"]
        {
            return Err("Не совпадает запись Epic в журнале".into());
        }
    }
    if let Some((p, _, _)) = &j.launcher {
        if *p != launcher_path() {
            return Err("Некорректный каталог Epic в журнале".into());
        }
    }
    Ok((stage, j))
}
pub(super) fn legacy_plan(path:&str)->Result<Plan,String>{
    let root=target(path)?;let (stage,j)=recovery(&root)?;
    let artifact=|name:&str|->Result<Vec<u8>,String>{
        let op=j.operations.iter().find(|o|o.name==name).ok_or("Нет метаданных сохранённой установки")?;
        let staged=stage.join("files").join(relative(name)?);let p=if staged.exists(){staged}else{root.join(relative(name)?)};no_links(&p)?;
        let bytes=fs::read(p).map_err(io)?;if op.new_hash.as_ref()!=Some(&digest(&bytes)){return Err("Метаданные сохранённой установки повреждены".into());}Ok(bytes)
    };
    let receipt:serde_json::Value=serde_json::from_slice(&artifact(".egstore/um-install.json")?).map_err(|_|"Повреждены компоненты установки")?;
    let raw=if let Some(op)=j.operations.iter().find(|o|o.name.starts_with(".egstore/um-")&&o.name.ends_with(".manifest")){artifact(&op.name)?}else{
        let path=PathBuf::from(receipt["manifest"].as_str().ok_or("Нет манифеста сохранённой установки")?);
        if path.parent()!=Some(root.join(".egstore").as_path()){return Err("Манифест находится вне движка".into());}no_links(&path)?;
        let bytes=fs::read(&path).map_err(io)?;if path.file_name().and_then(|n|n.to_str())!=Some(format!("um-{}.manifest",digest(&bytes)).as_str()){return Err("Манифест сохранённой установки повреждён".into());}bytes
    };
    let mut manifest=parse(&raw)?;if manifest.build_version_string!=j.build{return Err("Не совпадает сборка сохранённой установки".into());}
    let selected:BTreeSet<String>=receipt["tags"].as_array().ok_or("Нет компонентов установки")?.iter().map(|t|t.as_str().map(str::to_owned).ok_or("Некорректный компонент")).collect::<Result<_,_>>()?;
    let tags=super::manifest::tags(&raw)?;
    let source_base=format!("https://egdownload.fastly-edge.com/Builds/{}/Releases/CloudDir/",if manifest.app_name_string.starts_with("UE_4"){"UE4"}else{"UE5"});
    manifest.custom_fields.get_or_insert_with(Default::default).insert("SourceURL".into(),source_base.trim_end_matches('/').into());
    let files:BTreeMap<_,_>=manifest.files().into_iter().filter(|(name,_)|tags.get(name).is_some_and(|t|t.is_empty()||t.iter().any(|t|selected.contains(t)))).collect();
    let bytes=files.values().map(|f|f.size() as u64).sum();
    let id=stage.file_name().and_then(|n|n.to_str()).and_then(|n|n.get(32..)).filter(|s|uuid::Uuid::parse_str(s).is_ok()).map(str::to_owned).unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    let info=Preview{id,app_name:manifest.app_name_string.clone(),build:j.build,path:j.root.clone(),update:j.operations.iter().any(|o|o.old_hash.is_some()),components:selected.into_iter().map(|tag|Component{label:tag.clone(),tag,bytes:0,selected:true}).collect(),bytes,download_bytes:0,changed:0,added:0,removed:0,required_bytes:0,free_bytes:free(&root)?,conflicts:vec![]};
    let plan=Plan{resume_stage:Some(stage.clone()),catalog_item_id:None,remove_unselected:false,source_base,info,raw,files,old:BTreeMap::new(),item:j.item.as_ref().filter(|(_,before,_)|!before.is_null()).map(|(p,b,_)|(p.clone(),b.clone())),chunk_sizes:manifest.chunk_filesize_list.into_iter().map(|(g,n)|(g,n as u64)).collect(),tags};
    super::resume::validate_stage(&plan,&stage)?;Ok(plan)
}
#[tauri::command]
pub fn engine_pending() -> Vec<Pending> {
    let mut roots = managed_roots();
    roots.extend(
        items()
            .into_iter()
            .filter_map(|(_, v)| v["InstallLocation"].as_str().map(PathBuf::from)),
    );
    for parent in crate::common_engine_roots() {
        if let Ok(entries) = fs::read_dir(parent) {
            roots.extend(entries.flatten().map(|e| e.path()));
        }
    }
    let mut seen = BTreeSet::new();
    roots
        .into_iter()
        .filter(|p| seen.insert(p.to_string_lossy().to_lowercase()))
        .filter_map(|p| {
            let (_, j) = recovery(&p).ok()?;
            Some(Pending {
                path: p.to_string_lossy().into_owned(),
                build: j.build,
            })
        })
        .collect()
}
#[tauri::command]
pub async fn engine_recover(
    state: tauri::State<'_, EngineState>,
    vault: tauri::State<'_, crate::vault::VaultState>,
    path: String,
) -> Result<(), String> {
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true, Ordering::SeqCst) {
        return Err("Дождитесь текущей операции".into());
    }
    let _busy = Busy(&state.active);
    if vault.active.swap(true, Ordering::SeqCst) {
        return Err("Дождитесь текущей операции с хранилищем".into());
    }
    let _vault = Busy(&vault.active);
    tauri::async_runtime::spawn_blocking(move || {
        let root = target(&path)?;
        idle(&root)?;
        let _installation_lock = lock_installation(&root)?;
        let (stage, mut j) = recovery(&root)?;
        if j.status != "complete" {
            rollback(&root, &stage, &j.operations)?;
            update_item(&j.item, true)?;
            update_item(&j.launcher, true)?;
            j.status = "rolledBack".into();
            atomic(
                &stage.join("journal.json"),
                &serde_json::to_vec(&j).unwrap(),
            )?;
        }
        fs::remove_file(root.join(".um-engine-update.json")).map_err(io)
    })
    .await
    .map_err(|_| "Восстановление прервано")?
}

pub(super) fn launcher_path() -> PathBuf {
    PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into()))
        .join("Epic/UnrealEngineLauncher/LauncherInstalled.dat")
}
fn launcher_update(
    plan: &Plan,
) -> Result<Option<(PathBuf, serde_json::Value, serde_json::Value)>, String> {
    #[cfg(test)] { let _ = plan; return Ok(None); }
    #[cfg(not(test))] { Ok(Some(launcher_update_at(&launcher_path(),&plan.info.app_name,&plan.info.build,Path::new(&plan.info.path))?)) }
}
fn launcher_update_at(path:&Path,app_name:&str,build:&str,root:&Path)->Result<(PathBuf,serde_json::Value,serde_json::Value),String> {
    // LauncherInstalled.dat is also read by UnrealVersionSelector/DesktopPlatform.
    // New UM installations used to be omitted when no existing Epic .item existed.
    no_links(&path)?;
    let before: serde_json::Value = if path.exists() {
        serde_json::from_slice(&fs::read(&path).map_err(io)?)
            .map_err(|_| "Повреждён список установок Epic")?
    } else { serde_json::Value::Null };
    let mut after = if before.is_null() { serde_json::json!({"InstallationList":[]}) } else { before.clone() };
    let list = after["InstallationList"]
        .as_array_mut()
        .ok_or("Нет списка установок Epic")?;
    let mut count = 0;
    for entry in list.iter_mut() {
        if entry["AppName"] == app_name
            && entry["InstallLocation"]
                .as_str()
                .is_some_and(|p| same_installation_path(p,&root.to_string_lossy()))
        {
            entry["AppVersion"] = build.into();
            count += 1;
        }
    }
    if count == 0 {
        if list.iter().any(|e| e["AppName"] == app_name && e["InstallLocation"].as_str().is_some_and(|p|Path::new(p).is_dir())) {
            return Err("Эта версия уже зарегистрирована в другой папке. Существующая запись сохранена.".into());
        }
        list.retain(|e| e["AppName"] != app_name);
        list.push(serde_json::json!({"AppName":app_name,"InstallLocation":root.to_string_lossy(),"AppVersion":build,"NamespaceId":"ue","ArtifactId":app_name}));
    } else if count != 1 {
        return Err("Неоднозначная запись движка в списке Epic".into());
    }
    Ok((path.to_path_buf(), before, after))
}

pub(super) struct InstallationLock {file:Option<fs::File>,directory:PathBuf}
impl Drop for InstallationLock {
    fn drop(&mut self) {drop(self.file.take());let _=fs::remove_dir(&self.directory);}
}
pub(super) fn lock_installation(root: &Path) -> Result<InstallationLock, String> {
    no_links(root)?;
    let parent = root.parent().ok_or("Не найдена папка установки")?;
    fs::create_dir_all(parent).map_err(io)?;
    let canonical = if root.exists() {
        root.canonicalize().map_err(io)?
    } else {
        parent
            .canonicalize()
            .map_err(io)?
            .join(root.file_name().ok_or("Некорректная папка")?)
    };
    let locks = parent.join(".um-engine-locks");
    no_links(&locks)?;
    fs::create_dir_all(&locks).map_err(io)?;
    let path = locks.join(format!(
        "{}.lock",
        digest(canonical.to_string_lossy().to_lowercase().as_bytes())
    ));
    no_links(&path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x04000000); // FILE_FLAG_DELETE_ON_CLOSE
    }
    options.open(path).map(|file|InstallationLock{file:Some(file),directory:locks}).map_err(|_| {
        "С этой установкой уже работает другой экземпляр Unreal Manager, либо папка недоступна."
            .into()
    })
}
#[cfg(all(test, windows))]
mod lock_tests {
    use super::*;
    #[test]
    fn installation_lock_survives_directory_creation_and_releases() {
        let base =
            std::env::temp_dir().join(format!("um-engine-lock-test-{}", uuid::Uuid::new_v4()));
        let root = base.join("engine");
        let first = lock_installation(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        assert!(lock_installation(&root).is_err());
        drop(first);
        assert!(!base.join(".um-engine-locks").exists());
        let second = lock_installation(&root).unwrap();
        drop(second);
        assert!(!base.join(".um-engine-locks").exists());
        fs::remove_dir_all(base).unwrap();
    }
}


#[tauri::command]
pub async fn engine_register_managed(state:tauri::State<'_,EngineState>,vault:tauri::State<'_,crate::vault::VaultState>)->Result<usize,String>{
    let _update_operation=crate::app_updates::operation()?;
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции с движком.".into());}
 let _busy=Busy(&state.active);
 if vault.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции с хранилищем".into());}let _vault=Busy(&vault.active);
 tauri::async_runtime::spawn_blocking(repair_managed_registration).await.map_err(|e|e.to_string())?
}
pub(crate) fn repair_managed_registration()->Result<usize,String>{
 let mut count=0;
 for engine in crate::scan_engines(){
  let root=target(&engine.path)?;let receipt=root.join(".egstore/um-install.json");
  if !receipt.exists()||root.join(".um-engine-update.json").exists(){continue;}
  let _lock=lock_installation(&root)?;idle(&root)?;no_links(&receipt)?;
  let value:serde_json::Value=serde_json::from_slice(&fs::read(&receipt).map_err(io)?).map_err(|e|e.to_string())?;
  let app_name=value["appName"].as_str().ok_or("Нет идентификатора установки")?;
  if app_name!=format!("UE_{}",engine.version){return Err("Идентификатор установки не совпадает с версией движка".into());}
  let build=value["build"].as_str().ok_or("Нет версии установки")?;
  let (version,changelist)=version(&root)?;
  if !same_build(build,&version,&changelist){return Err("Версия движка не совпадает с записью установки".into());}
  let manifest_path=PathBuf::from(value["manifest"].as_str().ok_or("Нет манифеста установки")?);
  if manifest_path.parent()!=Some(root.join(".egstore").as_path()) || manifest_path.extension().and_then(|s|s.to_str())!=Some("manifest"){return Err("Манифест находится вне папки установки".into());}
  no_links(&manifest_path)?;let raw=fs::read(&manifest_path).map_err(io)?;
  if manifest_path.file_stem().and_then(|s|s.to_str())!=Some(format!("um-{}",digest(&raw)).as_str()){return Err("Манифест установки изменён".into());}
  let parsed=parse(&raw)?;if parsed.app_name_string!=app_name||parsed.build_version_string!=build{return Err("Манифест не совпадает с установленной сборкой".into());}
  let tags=manifest::tags(&raw)?;let selected:BTreeSet<_>=value["tags"].as_array().ok_or("Нет состава установки")?.iter().filter_map(|s|s.as_str()).collect();
  let installed_size:u64=parsed.file_manifest_list.iter().filter(|f|tags.get(&f.filename).is_some_and(|t|t.is_empty()||t.iter().any(|t|selected.contains(t.as_str())))).map(|f|f.size() as u64).sum();
  let existing=items().into_iter().find(|(_,item)|item["AppName"]==app_name&&item["InstallLocation"].as_str().is_some_and(|p|same_installation_path(p,&engine.path)));
  let item_update=if let Some((path,before))=existing {
    let mut after=before.clone();after["AppVersionString"]=build.into();after["CompleteManifestPath"]=manifest_path.to_string_lossy().into_owned().into();after["InstallSize"]=installed_size.into();after["bIsIncompleteInstall"]=false.into();Some((path,before,after))
  }else if let Some(catalog_id)=value["catalogItemId"].as_str(){let guid=uuid::Uuid::new_v4().simple().to_string().to_uppercase();Some((epic_item_directory().join(format!("{guid}.item")),serde_json::Value::Null,new_epic_item(app_name,catalog_id,build,&root,&manifest_path,installed_size,&guid)))}else{None};
  let launcher_update=launcher_update_at(&launcher_path(),app_name,build,&root)?;
  let changed=item_update.as_ref().is_some_and(|i|i.1!=i.2)||launcher_update.1!=launcher_update.2;
  if changed {
   let backup=crate::storage::root().join("Backups/engine-registration").join(format!("{}.json",uuid::Uuid::new_v4()));
   atomic(&backup,&serde_json::to_vec_pretty(&serde_json::json!({"item":item_update,"launcher":launcher_update})).map_err(|e|e.to_string())?)?;
   update_item(&item_update,false)?;
   if let Err(error)=update_item(&Some(launcher_update),false){let _=update_item(&item_update,true);return Err(error);}
   count+=1;
  }
 }
 Ok(count)
}

#[cfg(test)]
fn assemble_files(plan:&Plan,stage:&Path,ops:&[Operation],cancel:&AtomicBool,progress:impl FnMut(u64,u64,u64))->Result<(),String>{assemble_files_controlled(plan,stage,ops,cancel,None,progress)}
struct AssemblyTemporary(PathBuf);
impl Drop for AssemblyTemporary{fn drop(&mut self){let _=fs::remove_file(&self.0);}}
fn assemble_files_controlled(plan:&Plan,stage:&Path,ops:&[Operation],cancel:&AtomicBool,paused:Option<&AtomicBool>,mut progress:impl FnMut(u64,u64,u64))->Result<(),String>{
    let mut complete = 0;
    let mut last = Instant::now();
    // Keep reused blocks across neighboring files; Epic often interleaves them.
    // Bounded to 128 MiB rather than decompressing each block for every file.
    let mut cache = BlockCache::new(128 * 1024 * 1024);
    let files_total=ops.iter().filter(|o|o.new_hash.is_some()).count() as u64;
    let mut files_done=0u64;
    progress(0,0,files_total);
    for op in ops {
        pause_checkpoint(paused,||progress(complete,files_done,files_total));
        if cancel.load(Ordering::SeqCst) {return Err("Сборка остановлена. Готовые файлы сохранены.".into());}
        let Some(expected) = &op.new_hash else {
            continue;
        };
        let f = &plan.files[&op.name];
        let p = stage.join("files").join(relative(&op.name)?);
        no_links(&p)?;
        if p.exists() {
            if hash_file(&p)? != *expected {
                return Err("Повреждён собранный файл".into());
            }
            complete += f.size() as u64;
            files_done+=1;
            if last.elapsed()>Duration::from_millis(250){progress(complete,files_done,files_total);last=Instant::now();}
            continue;
        }
        fs::create_dir_all(p.parent().unwrap()).map_err(io)?;
        let temp = p.with_extension(format!("um-{}", uuid::Uuid::new_v4()));
        let mut out = std::io::BufWriter::with_capacity(256 * 1024, fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(io)?);
        let _temporary=AssemblyTemporary(temp.clone());
        let mut hash = Sha1::new();
        let mut written=0u64;
        for part in &f.file_chunk_parts {
            pause_checkpoint(paused,||progress(complete+written,files_done,files_total));
            if cancel.load(Ordering::SeqCst){return Err("Сборка остановлена. Готовые файлы сохранены.".into());}
            let data = cache.get(&stage, &part.guid)?;
            let slice = data
                .get(part.offset as usize..(part.offset + part.size) as usize)
                .ok_or("Размер блока не совпадает")?;
            out.write_all(slice).map_err(io)?;
            hash.update(slice);
            written+=slice.len() as u64;
            if last.elapsed()>Duration::from_millis(250){progress(complete+written,files_done,files_total);last=Instant::now();}
        }
        out.flush().map_err(io)?;
        out.get_ref().sync_data().map_err(io)?;
        drop(out);
        if format!("{:x}", hash.finalize()) != *expected {
            return Err("Контрольная сумма файла не совпала".into());
        }
        fs::rename(temp, p).map_err(io)?;
        complete += f.size() as u64;
        files_done+=1;
        if last.elapsed() > Duration::from_millis(250) {
            progress(complete,files_done,files_total);
            last = Instant::now();
        }
    }
    progress(complete,files_done,files_total);
    Ok(())
}

fn epic_item_directory()->PathBuf {PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(||"C:\\ProgramData".into())).join("Epic/EpicGamesLauncher/Data/Manifests")}
fn new_epic_item(app_name:&str,catalog_id:&str,build:&str,root:&Path,manifest:&Path,size:u64,guid:&str)->serde_json::Value {
 serde_json::json!({"FormatVersion":0,"AppName":app_name,"CatalogNamespace":"ue","CatalogItemId":catalog_id,"AppVersionString":build,"DisplayName":"Unreal Engine","InstallLocation":root,"CompleteManifestPath":manifest,"ManifestLocation":root.join(".egstore"),"StagingLocation":root.join(".egstore/bps"),"InstallationGuid":guid,"InstallSize":size,"LaunchExecutable":"Engine/Binaries/Win64/UnrealEditor.exe","LaunchCommand":"","AppCategories":["engines/ue5","engines"],"bIsApplication":false,"bIsExecutable":true,"bIsIncompleteInstall":false,"bNeedsValidation":false,"bRequiresAuth":true,"bCanRunOffline":false})
}

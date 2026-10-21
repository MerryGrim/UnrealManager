//! A short-lived, headless broker for allowlisted Epic UE registration metadata.
//! It never opens storage, an account session, Tauri, or a WebView.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{fs::{self, File, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, path::{Component, Path, PathBuf}, time::{Duration, Instant}};

const LIMIT: usize = 2 * 1024 * 1024;
const IPC_LIMIT: usize = 16 * 1024;
const CANCELLED: &str = "Запрос администратора отменён. Программа остаётся открытой.";
const HELPER_ERROR: &str = "Не удалось подтвердить разрешение на обновление записи Epic. Повторите операцию.";
fn user_error(path:&Path,details:String)->String{format!("UM_ELEVATION_REQUIRED:{}",serde_json::json!({"operation":"epicRecord","path":path,"message":HELPER_ERROR,"details":details.chars().take(8192).collect::<String>()}))}
fn error(e: impl std::fmt::Display) -> String { format!("Epic registration: {e}") }
fn walk_ancestors(path:&Path,mut inspect:impl FnMut(&Path)->Result<bool,String>)->Result<(),String>{
    for part in path.ancestors().collect::<Vec<_>>().into_iter().rev(){if !inspect(part)?{break;}}
    Ok(())
}
fn links(path: &Path) -> Result<(), String> {
    local_absolute(path)?; platform::local_drive(path)?;
    // Check the root and each ancestor before descending. A leaf-first probe
    // could follow a local junction to a remote share before finding the link.
    walk_ancestors(path,|part|{
        match fs::symlink_metadata(part) {
            Ok(meta)=>{
                #[cfg(windows)] let reparse={use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0};
                #[cfg(not(windows))] let reparse=meta.file_type().is_symlink();
                if reparse{return Err(error("reparse points are not permitted"));}
            }
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(false),
            Err(e)=>return Err(error(e)),
        }Ok(true)
    })
}
fn same(a: &Path, b: &Path) -> bool { normalized(a) == normalized(b) }
fn normalized(path: &Path) -> String { path.to_string_lossy().replace('/', "\\").trim_start_matches("\\\\?\\").to_lowercase() }
fn local_absolute(path: &Path) -> Result<(), String> {
    #[cfg(windows)] {
        use std::path::Prefix;
        if !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_))) {
            return Err(error("local disk path required"));
        }
    }
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir) || matches!(c,Component::Normal(v) if v==".." || v==".")) || path.to_string_lossy().contains('\0') {
        return Err(error("invalid absolute path"));
    }
    Ok(())
}
// Win32 DRIVE_REMOVABLE/FIXED/CDROM/RAMDISK. Unknown and remote mappings are
// rejected before opening a privileged path; this is not a hardware probe.
fn allowed_drive_kind(kind:u32)->bool {matches!(kind,2|3|5|6)}
fn absolute(path: &Path) -> Result<(), String> { links(path) }
fn uuid(id: &str) -> bool { uuid::Uuid::parse_str(id).is_ok_and(|x| x.to_string() == id) }
fn item_name(path: &Path) -> bool {
    path.file_name().and_then(|v| v.to_str()).is_some_and(|v| v.is_ascii() && v.len() == 37 && v[32..].eq_ignore_ascii_case(".item") && v[..32].bytes().all(|c| c.is_ascii_hexdigit()))
}
pub(crate) fn required(operation: &str, path: &Path) -> String {
    if is_elevated() { return format!("Недостаточно прав для операции с папкой: {}",path.display()); }
    format!("UM_ELEVATION_REQUIRED:{}", serde_json::json!({"operation":operation,"path":path,"message":"Для этой папки нужны права администратора. Выберите доступную папку."}))
}
pub(crate) fn is_elevated() -> bool { platform::elevated().unwrap_or(false) }

#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Job { schema: u32, nonce: String, parent_pid: u32, parent_created: u64, parent_exe: PathBuf, target: PathBuf, before: Value, after: Value, #[serde(default,skip_serializing_if="Option::is_none")] launcher: Option<Vec<LauncherEngine>> }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct LauncherEngine { pub root:PathBuf, pub app_name:String, pub build:String, pub manifest:PathBuf }
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Message { schema: u32, nonce: String, seq: u64, action: String }
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Reply { schema: u32, nonce: String, seq: u64, helper_pid: u32, ok: bool, error: Option<String> }

fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str,String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 32768 && !s.contains('\0')).ok_or_else(|| error(format!("invalid {key}")))
}
fn record(value: &Value) -> Result<PathBuf,String> {
    if !value.is_object() || string(value,"CatalogNamespace")? != "ue" { return Err(error("not an Unreal Engine record")); }
    let name=string(value,"AppName")?;
    if !name.starts_with("UE_") || name.len()>80 || !name.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)) { return Err(error("invalid UE application name")); }
    let root=PathBuf::from(string(value,"InstallLocation")?); absolute(&root)?;
    Ok(root)
}
fn fresh_record(value: &Value, target: &Path) -> Result<(),String> {
    let root=record(value)?;
    let object=value.as_object().ok_or_else(|| error("invalid new record"))?;
    let keys=["FormatVersion","AppName","CatalogNamespace","CatalogItemId","AppVersionString","DisplayName","InstallLocation","CompleteManifestPath","ManifestLocation","StagingLocation","InstallationGuid","InstallSize","LaunchExecutable","LaunchCommand","AppCategories","bIsApplication","bIsExecutable","bIsIncompleteInstall","bNeedsValidation","bRequiresAuth","bCanRunOffline"];
    if object.len()!=keys.len() || keys.iter().any(|k| !object.contains_key(*k)) { return Err(error("unexpected new record fields")); }
    let guid=string(value,"InstallationGuid")?;
    if guid.len()!=32 || !guid.bytes().all(|x| x.is_ascii_hexdigit()) || !target.file_stem().and_then(|x|x.to_str()).is_some_and(|x|x.eq_ignore_ascii_case(guid)) { return Err(error("registration GUID mismatch")); }
    if value["FormatVersion"]!=0 || value["DisplayName"]!="Unreal Engine" || value["LaunchExecutable"]!="Engine/Binaries/Win64/UnrealEditor.exe" || value["LaunchCommand"]!="" || value["AppCategories"]!=serde_json::json!(["engines/ue5","engines"]) || value["bIsApplication"]!=false || value["bIsExecutable"]!=true || value["bIsIncompleteInstall"]!=false || value["bNeedsValidation"]!=false || value["bRequiresAuth"]!=true || value["bCanRunOffline"]!=false || value["InstallSize"].as_u64().is_none() {
        return Err(error("invalid new UE record schema"));
    }
    if !same(Path::new(string(value,"ManifestLocation")?),&root.join(".egstore")) || !same(Path::new(string(value,"StagingLocation")?),&root.join(".egstore/bps")) { return Err(error("invalid UE metadata directories")); }
    string(value,"CatalogItemId")?; string(value,"AppVersionString")?;
    Ok(())
}
fn manifest(value: &Value) -> Result<(PathBuf,String),String> {
    let root=record(value)?;
    let path=PathBuf::from(string(value,"CompleteManifestPath")?); absolute(&path)?;
    let name=path.file_name().and_then(|n| n.to_str()).ok_or_else(||error("missing manifest filename"))?;
    let digest=name.strip_prefix("um-").and_then(|s|s.strip_suffix(".manifest")).filter(|s|s.len()==40 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))).ok_or_else(||error("invalid owned manifest name"))?;
    if !same(path.parent().ok_or_else(||error("manifest parent"))?,&root.join(".egstore")) { return Err(error("manifest outside engine metadata")); }
    Ok((path.clone(),digest.to_owned()))
}
fn validate(job: &Job, directory: &Path) -> Result<(),String> {
    if job.schema!=1 || !uuid(&job.nonce) || job.parent_pid==0 || job.parent_created==0 { return Err(error("invalid job identity")); }
    absolute(&job.parent_exe)?; absolute(&job.target)?; absolute(directory)?;
    if job.launcher.is_some() { return validate_launcher(job,directory); }
    if !item_name(&job.target) || !same(job.target.parent().ok_or_else(||error("target parent"))?,directory) { return Err(error("target outside Epic manifests")); }
    if job.before.is_null() && job.after.is_null() { return Err(error("empty change")); }
    if !job.before.is_null() { record(&job.before)?; }
    if !job.after.is_null() {
        record(&job.after)?; manifest(&job.after)?;
        if job.after["InstallSize"].as_u64().is_none() || job.after["bIsIncompleteInstall"]!=false { return Err(error("invalid completed metadata")); }
        if job.before.is_null() { fresh_record(&job.after,&job.target)?; }
        else {
            let mut old=job.before.as_object().ok_or_else(||error("invalid before"))?.clone();
            let mut new=job.after.as_object().ok_or_else(||error("invalid after"))?.clone();
            for key in ["AppVersionString","CompleteManifestPath","InstallSize","bIsIncompleteInstall"] { old.remove(key); new.remove(key); }
            if old!=new { return Err(error("change includes forbidden fields")); }
            string(&job.after,"AppVersionString")?;
        }
    }
    Ok(())
}
fn launcher_engine(engine:&LauncherEngine)->Result<(),String>{
    record(&serde_json::json!({"CatalogNamespace":"ue","AppName":engine.app_name,"InstallLocation":engine.root}))?;
    string(&serde_json::json!({"build":engine.build}),"build")?;
    manifest(&serde_json::json!({"CatalogNamespace":"ue","AppName":engine.app_name,"InstallLocation":engine.root,"CompleteManifestPath":engine.manifest}))?;
    Ok(())
}
// This reconstruction, rather than a generic before/after writer, limits the
// elevated authority to one version field or one exact new UE discovery row.
pub(crate) fn launcher_after(before:&Value,engines:&[LauncherEngine])->Result<Value,String>{
    if engines.is_empty() || engines.len()>128 {return Err(error("invalid managed engine count"));}
    let mut after=if before.is_null(){serde_json::json!({"InstallationList":[]})}else{before.clone()};
    let list=after.get_mut("InstallationList").and_then(Value::as_array_mut).filter(|v|v.len()<=8192).ok_or_else(||error("invalid Epic installation list"))?;
    let mut seen=std::collections::BTreeSet::new();
    for engine in engines {
        launcher_engine(engine)?;
        if !seen.insert((engine.app_name.clone(),normalized(&engine.root))){return Err(error("duplicate managed engine"));}
        let matching:Vec<_>=list.iter().enumerate().filter(|(_,entry)|entry["AppName"]==engine.app_name && entry["InstallLocation"].as_str().is_some_and(|p|normalized(Path::new(p)).trim_end_matches('\\')==normalized(&engine.root).trim_end_matches('\\'))).map(|(i,_)|i).collect();
        match matching.as_slice(){
            [index]=>list[*index]["AppVersion"]=engine.build.clone().into(),
            []=>{
                // Never remove another path, even a stale one, to gain authority
                // over an otherwise unrelated Epic installation.
                if list.iter().any(|entry|entry["AppName"]==engine.app_name){return Err(error("UE application is registered at another path"));}
                list.push(serde_json::json!({"AppName":engine.app_name,"InstallLocation":engine.root,"AppVersion":engine.build,"NamespaceId":"ue","ArtifactId":engine.app_name}));
            }
            _=>return Err(error("ambiguous Epic installation")),
        }
    }
    if list.len()>8192{return Err(error("Epic installation list limit"));}
    Ok(after)
}
fn validate_launcher(job:&Job,target:&Path)->Result<(),String>{
    if !same(&job.target,target) {return Err(error("target is not the fixed Epic installation list"));}
    let engines=job.launcher.as_deref().ok_or_else(||error("missing launcher intent"))?;
    if launcher_after(&job.before,engines)?!=job.after {return Err(error("change includes unrelated Epic installations or fields"));}
    Ok(())
}
fn verify_launcher_proof(engines:&[LauncherEngine])->Result<Vec<File>,String>{
    let mut guards=Vec::new();
    for engine in engines {
        launcher_engine(engine)?;
        let marker=engine.root.join(".um-engine-update.json");links(&marker)?;
        match fs::symlink_metadata(&marker){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>(),Err(e)=>return Err(error(e)),Ok(_)=>return Err(error("managed installation is incomplete"))}
        let mut receipt=input(&engine.root.join(".egstore/um-install.json"))?;
        let value=read_json(&mut receipt,LIMIT)?;
        if value["appName"]!=engine.app_name || value["build"]!=engine.build || !same(Path::new(string(&value,"manifest")?),&engine.manifest){return Err(error("managed receipt identity mismatch"));}
        let metadata=serde_json::json!({"CatalogNamespace":"ue","AppName":engine.app_name,"InstallLocation":engine.root,"CompleteManifestPath":engine.manifest});
        let mut file=verify_manifest(&metadata)?.ok_or_else(||error("missing managed manifest"))?;
        file.seek(SeekFrom::Start(0)).map_err(error)?;let mut raw=Vec::new();file.read_to_end(&mut raw).map_err(error)?;
        let (app,build)=crate::engine::install::registration_manifest_identity(&raw)?;
        if app!=engine.app_name || build!=engine.build{return Err(error("managed manifest identity mismatch"));}
        let mut version=input(&engine.root.join("Engine/Build/Build.version"))?;let v=read_json(&mut version,LIMIT)?;
        let number=|key:&str|v[key].as_u64().ok_or_else(||error("invalid engine build version"));
        let semantic=format!("{}.{}.{}",number("MajorVersion")?,number("MinorVersion")?,number("PatchVersion")?);
        let changelist=number("Changelist")?.to_string();
        if engine.app_name!=format!("UE_{}.{}",number("MajorVersion")?,number("MinorVersion")?) || !engine.build.split_once('-').is_some_and(|(version,rest)|version==semantic && rest.split('+').next()==Some(changelist.as_str())){return Err(error("installed build identity mismatch"));}
        guards.extend([receipt,file,version]);
    }
    Ok(guards)
}
fn input(path: &Path) -> Result<File,String> {
    links(path)?;
    let mut opts=OpenOptions::new(); opts.read(true);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; opts.share_mode(1); }
    let file=opts.open(path).map_err(error)?;
    if !file.metadata().map_err(error)?.is_file() { return Err(error("not a regular file")); }
    platform::one_link(&file)?;
    Ok(file)
}
fn read_json(file: &mut File, limit: usize) -> Result<Value,String> {
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut data=Vec::new(); file.take(limit as u64+1).read_to_end(&mut data).map_err(error)?;
    if data.len()>limit { return Err(error("JSON size limit")); }
    serde_json::from_slice(&data).map_err(|_|error("invalid JSON"))
}
fn current(path: &Path) -> Result<Value,String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(Value::Null),
        Err(e)=>Err(error(e)),
        Ok(_)=>read_json(&mut input(path)?,LIMIT),
    }
}
fn expected(job: &Job) -> Result<Value,String> {
    let value=current(&job.target)?;
    if value!=job.before && value!=job.after { return Err(error("Epic record changed outside this operation")); }
    Ok(value)
}
fn initial(job: &Job) -> Result<(),String> {
    let observed=expected(job)?;
    if let Some(engines)=&job.launcher {
        if observed!=job.before{return Err(error("Epic installation list changed before authorization"));}
        verify_launcher_proof(engines)?;
    }
    // An absent record is not authority to recreate arbitrary legacy fields.
    // Delete/restore is authorized only while the actual original exists.
    if job.after.is_null() && observed!=job.before { return Err(error("deletion record no longer exists")); }
    Ok(())
}
fn parent_matches(job:&Job, actual:(u64,PathBuf), own_exe:&Path)->Result<(),String>{
    if actual.0!=job.parent_created || !same(&actual.1,&job.parent_exe) || !same(&actual.1,own_exe) {return Err(error("stale or foreign parent"));}
    Ok(())
}
fn next_message(message:&Message,job:&Job,seq:u64)->Result<bool,String>{
    if message.schema!=1 || message.nonce!=job.nonce || message.seq>seq.saturating_add(1) {return Err(error("broker request identity"));}
    if !["check","apply","restore","finish"].contains(&message.action.as_str()) {return Err(error("unsupported broker action"));}
    Ok(message.seq==seq.saturating_add(1))
}
fn verify_manifest(after: &Value) -> Result<Option<File>,String> {
    if after.is_null() { return Ok(None); }
    let (path,digest)=manifest(after)?; let mut file=input(&path)?;
    if file.metadata().map_err(error)?.len()>512*1024*1024 { return Err(error("manifest size limit")); }
    let mut hasher=Sha1::new(); let mut buffer=vec![0u8;65536];
    loop { let n=file.read(&mut buffer).map_err(error)?; if n==0 { break; } hasher.update(&buffer[..n]); }
    if format!("{:x}",hasher.finalize())!=digest { return Err(error("manifest digest mismatch")); }
    Ok(Some(file))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(),String> {
    links(path)?; let mut f=OpenOptions::new().write(true).create_new(true).open(path).map_err(error)?;
    f.write_all(bytes).map_err(error)?; f.sync_all().map_err(error)
}
fn raw_digest(path:&Path)->Result<String,String>{let file=input(path)?;let mut data=Vec::new();file.take(LIMIT as u64+1).read_to_end(&mut data).map_err(error)?;if data.len()>LIMIT{return Err(error("metadata size limit"))}Ok(format!("{:x}",Sha1::digest(data)))}
fn replace(job: &Job, restore: bool) -> Result<(),String> {
    let desired=if restore {&job.before} else {&job.after};
    let observed=expected(job)?;
    if &observed==desired { return Ok(()); }
    let _proof=if let Some(engines)=&job.launcher {verify_launcher_proof(engines)?}else if !restore {verify_manifest(desired)?.into_iter().collect()}else{Vec::new()};
    // File locks are released before ReplaceFile; holding a DELETE-denying
    // handle here would turn a permitted replacement into a sharing failure.
    if desired.is_null() {
        if current(&job.target)?!=observed { return Err(error("record changed before deletion")); }
        links(&job.target)?; fs::remove_file(&job.target).map_err(error)?;
        return Ok(());
    }
    let old_digest=if observed.is_null(){None}else{Some(raw_digest(&job.target)?)};
    let parent=job.target.parent().ok_or_else(||error("target parent"))?;
    let temporary=parent.join(format!(".um-{}-{}.tmp",job.nonce,uuid::Uuid::new_v4()));
    let backup=parent.join(format!(".um-{}-{}.bak",job.nonce,uuid::Uuid::new_v4()));
    write_new(&temporary,&serde_json::to_vec(desired).map_err(error)?)?;
    if current(&job.target)?!=observed { let _=fs::remove_file(&temporary); return Err(error("record changed before replacement")); }
    links(parent)?; links(&job.target)?;
    let result=platform::replace(&job.target,&temporary,&backup,!observed.is_null());
    if let Err(cause)=result {
        // ReplaceFile can move its inputs even when it returns failure. Only
        // restore a verified old backup into an absent target; never replace
        // an unknown external edit or delete the sole remaining input.
        if !observed.is_null() && current(&job.target).ok()==Some(Value::Null) && current(&backup).ok()==Some(observed.clone()) && raw_digest(&backup).ok()==old_digest {
            let _=platform::move_new(&backup,&job.target);
        }
        return Err(error(format!("{cause}; retained target={}, temporary={}, backup={}",job.target.display(),temporary.display(),backup.display())));
    }
    if current(&job.target)?!=*desired { return Err(error("replacement verification failed; backup retained")); }
    if backup.exists() {
        if current(&backup)?!=observed || Some(raw_digest(&backup)?)!=old_digest { return Err(error("unexpected backup; retained")); }
        links(&backup)?; fs::remove_file(&backup).map_err(error)?;
    }
    Ok(())
}

struct Mailbox { file: File }
impl Mailbox {
    fn open(path: &Path) -> Result<Self,String> {
        links(path)?; let mut o=OpenOptions::new(); o.read(true).write(true);
        #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; o.share_mode(3); }
        let file=o.open(path).map_err(error)?; platform::one_link(&file)?; Ok(Self{file})
    }
    fn read<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>,String> {
        let _lock=platform::FileLock::new(&self.file,false)?;
        if self.file.metadata().map_err(error)?.len()==0 { return Ok(None); }
        let value=read_json(&mut self.file,IPC_LIMIT)?;
        serde_json::from_value(value).map(Some).map_err(|_|error("invalid broker message"))
    }
    fn write(&mut self, value: &impl Serialize) -> Result<(),String> {
        let bytes=serde_json::to_vec(value).map_err(error)?;
        if bytes.len()>IPC_LIMIT { return Err(error("message size limit")); }
        let _lock=platform::FileLock::new(&self.file,true)?;
        self.file.seek(SeekFrom::Start(0)).map_err(error)?; self.file.write_all(&bytes).map_err(error)?;
        self.file.set_len(bytes.len() as u64).map_err(error)?; self.file.sync_all().map_err(error)
    }
}

pub(crate) struct RegistrationBroker {
    process: platform::Process, nonce: String, seq: u64, target:PathBuf,
    command: Mailbox, response: Mailbox, _job: File,
    _cleanup: Cleanup,
}
struct Cleanup {path:PathBuf,keep:bool}
impl Drop for Cleanup { fn drop(&mut self) { if !self.keep {cleanup_flat(&self.path);} } }
impl RegistrationBroker {
    fn request(&mut self, action: &str) -> Result<(),String> {
        if !self.process.alive()? { return Err(HELPER_ERROR.into()); }
        self.seq=self.seq.checked_add(1).ok_or_else(||error("sequence overflow"))?;
        self.command.write(&Message{schema:1,nonce:self.nonce.clone(),seq:self.seq,action:action.into()})?;
        let start=Instant::now();
        while start.elapsed()<Duration::from_secs(30) {
            if let Some(reply)=self.response.read::<Reply>()? {
                if reply.schema!=1 || reply.nonce!=self.nonce || reply.helper_pid!=self.process.id { return Err(HELPER_ERROR.into()); }
                if reply.seq==self.seq { return if reply.ok {Ok(())} else {Err(reply.error.unwrap_or_else(||HELPER_ERROR.into()))}; }
                if reply.seq>self.seq { return Err(error("unexpected response sequence")); }
            }
            if !self.process.alive()? { return Err(HELPER_ERROR.into()); }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(HELPER_ERROR.into())
    }
    pub(crate) fn check(&mut self) -> Result<(),String> { let r=self.request("check");if r.is_err(){self._cleanup.keep=true;}r.map_err(|e|user_error(&self.target,e)) }
    pub(crate) fn update(&mut self, restore: bool) -> Result<(),String> { let r=self.request(if restore {"restore"} else {"apply"});if r.is_err(){self._cleanup.keep=true;}r.map_err(|e|format!("Ошибка регистрации Epic: {}: {e}",self.target.display())) }
}
impl Drop for RegistrationBroker {
    fn drop(&mut self) {
        let _=self.request("finish");
        if self.process.wait(Duration::from_secs(3)).is_err() { let _=self.process.terminate(); }
        // Fields drop in declaration order. Cleanup runs only after the child
        // and all immutable/mailbox handles have closed.
    }
}
fn cleanup_flat(directory: &Path) {
    if links(directory).is_err() { return; }
    let Ok(entries)=fs::read_dir(directory) else {return};
    let entries:Vec<_>=entries.filter_map(Result::ok).collect();
    if entries.iter().any(|e| !["job.json","ready.json","command.json","response.json"].iter().any(|n|e.file_name()==*n) || links(&e.path()).is_err() || !e.file_type().is_ok_and(|t|t.is_file())) {return;}
    for entry in entries { let _=fs::remove_file(entry.path()); }
    let _=fs::remove_dir(directory);
}
pub(crate) fn prepare(change: &(PathBuf,Value,Value)) -> Result<RegistrationBroker,String> {
    prepare_inner(change,None).map_err(|e|if e==CANCELLED || e.starts_with("UM_ELEVATION_REQUIRED:"){e}else{user_error(&change.0,e)})
}
pub(crate) fn launcher_path()->Result<PathBuf,String>{platform::launcher_path()}
pub(crate) fn prepare_launcher(change:&(PathBuf,Value,Value),engines:Vec<LauncherEngine>)->Result<RegistrationBroker,String>{
    prepare_inner(change,Some(engines)).map_err(|e|if e==CANCELLED || e.starts_with("UM_ELEVATION_REQUIRED:"){e}else{user_error(&change.0,e)})
}
fn prepare_inner(change: &(PathBuf,Value,Value),launcher:Option<Vec<LauncherEngine>>) -> Result<RegistrationBroker,String> {
    let nonce=uuid::Uuid::new_v4().to_string();
    let parent=platform::Process::open(std::process::id())?;
    let (created,exe)=parent.identity()?;
    let job=Job{schema:1,nonce:nonce.clone(),parent_pid:parent.id,parent_created:created,parent_exe:exe,target:change.0.clone(),before:change.1.clone(),after:change.2.clone(),launcher};
    validate(&job,&if job.launcher.is_some(){platform::launcher_path()?}else{platform::epic_directory()?})?;
    let encoded=serde_json::to_vec(&job).map_err(error)?;
    if encoded.len()>LIMIT{return Err(error("registration job size limit"));}
    let directory=std::env::temp_dir().join(format!("um-epic-registration-{nonce}"));
    absolute(&directory)?; fs::create_dir(&directory).map_err(error)?;
    let mut cleanup=Some(Cleanup{path:directory.clone(),keep:false});
    let result=(|| {
        platform::secure_directory(&directory,&parent.sid()?)?;
        write_new(&directory.join("job.json"),&encoded)?;
        for name in ["ready.json","command.json","response.json"] {write_new(&directory.join(name),b"")?;}
        let guard=input(&directory.join("job.json"))?;
        let mut ready=Mailbox::open(&directory.join("ready.json"))?;
        let command=Mailbox::open(&directory.join("command.json"))?;
        let response=Mailbox::open(&directory.join("response.json"))?;
        let process=platform::launch(&job.parent_exe,&directory.join("job.json"))?;
        let mut broker=RegistrationBroker{process,nonce,seq:0,target:job.target.clone(),command,response,_job:guard,_cleanup:cleanup.take().expect("local cleanup owner")};
        let start=Instant::now();
        loop {
            if let Some(r)=ready.read::<Reply>()? {
                if r.schema!=1 || r.nonce!=broker.nonce || r.helper_pid!=broker.process.id || r.seq!=0 {return Err(HELPER_ERROR.into());}
                if !r.ok {broker._cleanup.keep=true;return Err(r.error.unwrap_or_else(||HELPER_ERROR.into()));}
                break;
            }
            if !broker.process.alive()? || start.elapsed()>Duration::from_secs(30) {return Err(HELPER_ERROR.into());}
            std::thread::sleep(Duration::from_millis(50));
        }
        broker.check()?; Ok(broker)
    })();
    result
}

pub(crate) fn run(path: PathBuf) -> Result<(),String> {
    if !platform::elevated()? {return Err(error("administrator token required"));}
    absolute(&path)?;
    if path.file_name().is_none_or(|n| n!="job.json") {return Err(error("invalid job filename"));}
    let directory=path.parent().ok_or_else(||error("job directory"))?;
    let mut guard=input(&path)?;
    let job:Job=serde_json::from_value(read_json(&mut guard,LIMIT)?).map_err(|_|error("invalid job"))?;
    if directory.file_name().is_none_or(|n|n!=format!("um-epic-registration-{}",job.nonce).as_str()) {return Err(error("job directory identity"));}
    validate(&job,&if job.launcher.is_some(){platform::launcher_path()?}else{platform::epic_directory()?})?;
    let parent=platform::Process::open(job.parent_pid)?;
    let (created,exe)=parent.identity()?;
    parent_matches(&job,(created,exe.clone()),&fs::canonicalize(std::env::current_exe().map_err(error)?).map_err(error)?)?;
    links(&exe)?; platform::verify_directory(directory,&parent.sid()?)?;
    for name in ["job.json","ready.json","command.json","response.json"] {platform::verify_private_file(&directory.join(name),&parent.sid()?)?;}
    let mut ready=Mailbox::open(&directory.join("ready.json"))?;
    let mut command=Mailbox::open(&directory.join("command.json"))?;
    let mut response=Mailbox::open(&directory.join("response.json"))?;
    let initial=(|| {initial(&job)?; platform::probe(&job.target)?; Ok::<_,String>(())})();
    let accepted=initial.is_ok();
    ready.write(&Reply{schema:1,nonce:job.nonce.clone(),seq:0,helper_pid:std::process::id(),ok:accepted,error:initial.err()})?;
    if !accepted {return Ok(());}
    // Parent lifetime is authoritative; an idle broker cannot become a daemon.
    let start=Instant::now(); let mut seq=0;
    while start.elapsed()<Duration::from_secs(24*60*60) && parent.alive()? {
        if let Some(message)=command.read::<Message>()? {
            if next_message(&message,&job,seq)? {
                seq=message.seq;
                let result=match message.action.as_str() {"check"=>expected(&job).and_then(|_|{if let Some(engines)=&job.launcher{verify_launcher_proof(engines)?;}platform::probe(&job.target)}),"apply"=>replace(&job,false),"restore"=>replace(&job,true),"finish"=>Ok(()),_=>Err(error("unsupported broker action"))};
                response.write(&Reply{schema:1,nonce:job.nonce.clone(),seq,helper_pid:std::process::id(),ok:result.is_ok(),error:result.err()})?;
                if message.action=="finish" {break;}
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

#[cfg(any(test,feature="engine-test-cli"))]
fn fixture_record(root:&Path,target:&Path,digest:&str)->Value {
    serde_json::json!({"FormatVersion":0,"AppName":"UE_0.0","CatalogNamespace":"ue","CatalogItemId":"00000000000000000000000000000000","AppVersionString":"QA0","DisplayName":"Unreal Engine","InstallLocation":root,"CompleteManifestPath":root.join(format!(".egstore/um-{digest}.manifest")),"ManifestLocation":root.join(".egstore"),"StagingLocation":root.join(".egstore/bps"),"InstallationGuid":target.file_stem().unwrap().to_string_lossy(),"InstallSize":1,"LaunchExecutable":"Engine/Binaries/Win64/UnrealEditor.exe","LaunchCommand":"","AppCategories":["engines/ue5","engines"],"bIsApplication":false,"bIsExecutable":true,"bIsIncompleteInstall":false,"bNeedsValidation":false,"bRequiresAuth":true,"bCanRunOffline":false})
}

/// Explicit test build only: one fresh protected UE_0.0 registration, one UAC
/// prompt, real apply/restore, and exact cleanup. No existing item is selected.
#[cfg(all(feature="engine-test-cli",windows))]
pub(crate) fn probe(report:PathBuf)->Result<(),String>{
    absolute(&report)?;
    if report.exists() {return Err(error("probe requires a fresh local report"));}
    let parent=report.parent().ok_or_else(||error("probe report parent"))?;
    if !parent.is_dir(){return Err(error("probe report parent must exist"));}
    let root=parent.join(format!("uac-registration-fixture-{}",uuid::Uuid::new_v4()));
    fs::create_dir(&root).map_err(error)?;fs::create_dir(root.join(".egstore")).map_err(error)?;
    let raw=b"Unreal Manager isolated Epic registration UAC probe";
    let digest=format!("{:x}",Sha1::digest(raw));
    let manifest=root.join(format!(".egstore/um-{digest}.manifest"));write_new(&manifest,raw)?;
    let target=platform::epic_directory()?.join(format!("{}.item",uuid::Uuid::new_v4().simple()));
    let before=fixture_record(&root,&target,&digest);let mut after=before.clone();after["AppVersionString"]="QA1".into();
    let mut broker=None;let mut checks=Vec::new();
    let outcome=(||{
        write_new(&target,&serde_json::to_vec(&before).map_err(error)?)?;
        platform::protect_probe(&target)?;let protected=platform::dacl(&target)?;
        if !platform::delete_denied(&target)? {return Err(error("probe requires standard-user DELETE denial"));}
        checks.push("owned protected item created");
        broker=Some(prepare(&(target.clone(),before.clone(),after.clone()))?);
        let b=broker.as_mut().unwrap();b.check()?;checks.push("elevated helper ready and rights checked");
        b.update(false)?;
        if current(&target)?!=after || platform::dacl(&target)?!=protected || !platform::delete_denied(&target)? {return Err(error("apply or DACL verification failed"));}
        checks.push("real ReplaceFile applied and DACL preserved");
        b.update(true)?;
        if current(&target)?!=before || platform::dacl(&target)?!=protected || !platform::delete_denied(&target)? {return Err(error("restore or DACL verification failed"));}
        checks.push("real ReplaceFile restored and DACL preserved");Ok::<_,String>(())
    })();
    let mut cleanup_error=None;
    if current(&target).ok()==Some(after.clone()) {
        if let Some(b)=broker.as_mut(){if let Err(e)=b.update(true){cleanup_error=Some(e);}}
    }
    drop(broker);
    if current(&target).ok().is_some_and(|v|v==before || v==after) {
        // This is our freshly created probe GUID only. Its protected DACL
        // retained an explicit original-user WRITE_DAC ACE across ReplaceFile.
        // Grant that user DELETE for cleanup; inherited Epic ACLs need not.
        let cleanup=(||{let sid=platform::Process::open(std::process::id())?.sid()?;platform::secure_directory(&target,&sid)?;if current(&target)?!=before && current(&target)?!=after{return Err(error("probe changed before cleanup"));}fs::remove_file(&target).map_err(error)})();
        if let Err(e)=cleanup{cleanup_error=Some(e);}else{cleanup_error=None;}
    } else if target.exists(){cleanup_error=Some(error("unknown probe item retained"));}
    if !target.exists(){let _=fs::remove_file(&manifest);let _=fs::remove_dir(root.join(".egstore"));let _=fs::remove_dir(&root);}
    let result=serde_json::json!({"schema":1,"purpose":"isolated fixed-scope Epic metadata UAC apply/restore; no real engine/account","checks":checks,"passed":outcome.is_ok()&&cleanup_error.is_none(),"error":outcome.as_ref().err(),"cleanupError":cleanup_error,"target":target,"targetAbsent":!target.exists(),"fixtureAbsent":!root.exists()});
    write_new(&report,&serde_json::to_vec_pretty(&result).map_err(error)?)?;
    outcome?;if !result["passed"].as_bool().unwrap_or(false){return Err(error("probe cleanup incomplete; see report"));}Ok(())
}
#[cfg(all(feature="engine-test-cli",not(windows)))]
pub(crate) fn probe(_:PathBuf)->Result<(),String>{Err(error("Windows required"))}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture{root:PathBuf}
    impl Fixture{
        fn new()->Self{let parent=std::env::var_os("UM_TEMP_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);fs::create_dir_all(&parent).unwrap();let root=parent.join(format!("um-registration-test-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();let this=Self{root};fs::create_dir(this.root.join("engine")).unwrap();fs::create_dir(this.root.join("engine/.egstore")).unwrap();this}
        fn job(&self)->Job{let raw=b"isolated manifest";let digest=format!("{:x}",Sha1::digest(raw));let target=self.root.join(format!("{}.item",uuid::Uuid::new_v4().simple()));let manifest=self.root.join(format!("engine/.egstore/um-{digest}.manifest"));if !manifest.exists(){write_new(&manifest,raw).unwrap();}let after=fixture_record(&self.root.join("engine"),&target,&digest);Job{schema:1,nonce:uuid::Uuid::new_v4().to_string(),parent_pid:1,parent_created:1,parent_exe:std::env::current_exe().unwrap(),target,before:Value::Null,after,launcher:None}}
        fn launcher(&self)->Job{
            let mut job=self.job();let root=self.root.join("engine");
            let mut metadata=vec![0u8;10];
            for value in ["UE_5.8","5.8.4-42+fixture"]{metadata.extend_from_slice(&((value.len()+1)as u32).to_le_bytes());metadata.extend_from_slice(value.as_bytes());metadata.push(0);}
            let mut payload=((metadata.len()+4)as u32).to_le_bytes().to_vec();payload.extend(metadata);
            let mut raw=Vec::new();for value in [1153351692u32,41,payload.len()as u32,payload.len()as u32]{raw.extend_from_slice(&value.to_le_bytes());}raw.extend_from_slice(&Sha1::digest(&payload));raw.push(0);raw.extend_from_slice(&0u32.to_le_bytes());raw.extend(payload);
            let manifest=root.join(format!(".egstore/um-{:x}.manifest",Sha1::digest(&raw)));write_new(&manifest,&raw).unwrap();
            fs::create_dir_all(root.join("Engine/Build")).unwrap();
            write_new(&root.join("Engine/Build/Build.version"),br#"{"MajorVersion":5,"MinorVersion":8,"PatchVersion":4,"Changelist":42}"#).unwrap();
            write_new(&root.join(".egstore/um-install.json"),&serde_json::to_vec(&serde_json::json!({"appName":"UE_5.8","build":"5.8.4-42+fixture","manifest":manifest,"tags":[],"backup":null})).unwrap()).unwrap();
            let engines=vec![LauncherEngine{root,app_name:"UE_5.8".into(),build:"5.8.4-42+fixture".into(),manifest}];
            job.target=self.root.join("LauncherInstalled.dat");
            job.before=serde_json::json!({"InstallationList":[{"AppName":"OtherGame","InstallLocation":"foreign-preserved","FutureGameField":{"keep":true}},{"AppName":"UE_5.8","InstallLocation":engines[0].root,"AppVersion":"5.8.3-41","FutureEngineField":[1,2]}],"FutureTopLevel":{"preserve":[3,4]}});
            job.after=launcher_after(&job.before,&engines).unwrap();job.launcher=Some(engines);job
        }
    }
    impl Drop for Fixture{fn drop(&mut self){if links(&self.root).is_ok(){let _=fs::remove_dir_all(&self.root);}}}
    #[test]fn fixed_scope_and_new_record_schema(){let f=Fixture::new();let mut j=f.job();assert!(validate(&j,&f.root).is_ok());j.target=f.root.join("outside").join(j.target.file_name().unwrap());assert!(validate(&j,&f.root).is_err());j=f.job();j.after["LaunchCommand"]="--arbitrary".into();assert!(validate(&j,&f.root).is_err());j=f.job();j.after["extra"]="value".into();assert!(validate(&j,&f.root).is_err());assert!(!item_name(Path::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé.item")));}
    #[test]fn updates_preserve_all_other_official_fields(){let f=Fixture::new();let mut j=f.job();j.before=j.after.clone();j.before["OfficialFutureField"]=serde_json::json!({"preserve":true});j.after=j.before.clone();j.after["AppVersionString"]="QA1".into();assert!(validate(&j,&f.root).is_ok());j.after["LaunchExecutable"]="else.exe".into();assert!(validate(&j,&f.root).is_err());}
    #[test]fn scope_nonce_parent_and_sequence_are_bound(){let f=Fixture::new();let j=f.job();assert!(parent_matches(&j,(j.parent_created,j.parent_exe.clone()),&j.parent_exe).is_ok());assert!(parent_matches(&j,(2,j.parent_exe.clone()),&j.parent_exe).is_err());let mut m=Message{schema:1,nonce:j.nonce.clone(),seq:1,action:"apply".into()};assert!(next_message(&m,&j,0).unwrap());assert!(!next_message(&m,&j,1).unwrap());m.nonce=uuid::Uuid::new_v4().to_string();assert!(next_message(&m,&j,0).is_err());m.nonce=j.nonce.clone();m.seq=3;assert!(next_message(&m,&j,0).is_err());m.seq=1;m.action="arbitrary".into();assert!(next_message(&m,&j,0).is_err());}
    #[test]fn real_apply_restore_and_external_edit_refusal(){let f=Fixture::new();let j=f.job();validate(&j,&f.root).unwrap();initial(&j).unwrap();replace(&j,false).unwrap();assert_eq!(current(&j.target).unwrap(),j.after);replace(&j,true).unwrap();assert!(!j.target.exists());replace(&j,false).unwrap();let foreign=serde_json::json!({"external":"keep"});fs::write(&j.target,serde_json::to_vec(&foreign).unwrap()).unwrap();assert!(replace(&j,true).is_err());assert_eq!(current(&j.target).unwrap(),foreign);}
    #[test]fn protected_existing_replacement_preserves_dacl(){let f=Fixture::new();let mut j=f.job();j.before=j.after.clone();j.after["AppVersionString"]="QA1".into();write_new(&j.target,&serde_json::to_vec(&j.before).unwrap()).unwrap();#[cfg(windows)]let acl=platform::dacl(&j.target).unwrap();replace(&j,false).unwrap();assert_eq!(current(&j.target).unwrap(),j.after);#[cfg(windows)]assert_eq!(platform::dacl(&j.target).unwrap(),acl);replace(&j,true).unwrap();assert_eq!(current(&j.target).unwrap(),j.before);#[cfg(windows)]assert_eq!(platform::dacl(&j.target).unwrap(),acl);}
    #[test]fn deletion_cannot_authorize_forged_absent_legacy_record(){let f=Fixture::new();let mut j=f.job();j.before=j.after.clone();j.after=Value::Null;j.before["LaunchExecutable"]="untrusted.exe".into();validate(&j,&f.root).unwrap();assert!(initial(&j).is_err());write_new(&j.target,&serde_json::to_vec(&j.before).unwrap()).unwrap();initial(&j).unwrap();replace(&j,false).unwrap();assert!(!j.target.exists());replace(&j,true).unwrap();assert_eq!(current(&j.target).unwrap(),j.before);}
    #[test]fn manifest_corruption_fails_before_item_mutation(){let f=Fixture::new();let j=f.job();let (path,_)=manifest(&j.after).unwrap();fs::write(path,b"corrupt").unwrap();assert!(replace(&j,false).is_err());assert!(!j.target.exists());}
    #[test]fn broker_is_send_and_cancellation_is_distinct(){fn assert_send<T:Send>(){}assert_send::<RegistrationBroker>();assert!(!CANCELLED.starts_with("UM_ELEVATION_REQUIRED:"));assert_ne!(CANCELLED,HELPER_ERROR);}
    #[test]fn launcher_intent_preserves_foreign_entries_and_unknown_fields(){
        let f=Fixture::new();let mut j=f.launcher();validate(&j,&j.target).unwrap();
        assert_eq!(j.after["InstallationList"][0],j.before["InstallationList"][0]);assert_eq!(j.after["FutureTopLevel"],j.before["FutureTopLevel"]);assert_eq!(j.after["InstallationList"][1]["FutureEngineField"],serde_json::json!([1,2]));
        j.after["InstallationList"][0]["FutureGameField"]["keep"]=false.into();assert!(validate(&j,&j.target).is_err());
        j.after=launcher_after(&j.before,j.launcher.as_ref().unwrap()).unwrap();j.after["FutureTopLevel"]=Value::Null;assert!(validate(&j,&j.target).is_err());
        j.after=Value::Null;assert!(validate(&j,&j.target).is_err());
        let mut j=f.job();j.launcher=Some(vec![]);assert!(validate(&j,&f.root.join("LauncherInstalled.dat")).is_err());
    }
    #[test]fn launcher_new_row_schema_refuses_other_paths_and_ambiguous_records(){
        let f=Fixture::new();let j=f.launcher();let engines=j.launcher.as_ref().unwrap();
        let fresh=launcher_after(&Value::Null,engines).unwrap();assert_eq!(fresh["InstallationList"][0].as_object().unwrap().len(),5);assert_eq!(fresh["InstallationList"][0]["NamespaceId"],"ue");
        let mut before=j.before.clone();before["InstallationList"][1]["InstallLocation"]=f.root.join("unrelated-even-if-missing").to_string_lossy().into_owned().into();assert!(launcher_after(&before,engines).is_err());
        before=j.before.clone();let duplicate=before["InstallationList"][1].clone();before["InstallationList"].as_array_mut().unwrap().push(duplicate);assert!(launcher_after(&before,engines).is_err());
        assert!(launcher_after(&serde_json::json!({"InstallationList":{}}),engines).is_err());
        assert!(launcher_after(&Value::Null,&[]).is_err());
    }
    #[test]fn launcher_real_apply_restore_cas_and_fixed_target(){
        let f=Fixture::new();let mut j=f.launcher();write_new(&j.target,&serde_json::to_vec(&j.before).unwrap()).unwrap();
        validate(&j,&j.target).unwrap();assert!(validate(&j,&f.root.join("other.dat")).is_err());initial(&j).unwrap();
        replace(&j,false).unwrap();assert_eq!(current(&j.target).unwrap(),j.after);replace(&j,true).unwrap();assert_eq!(current(&j.target).unwrap(),j.before);
        let mut external=j.before.clone();external["EpicExternalEdit"]=true.into();fs::write(&j.target,serde_json::to_vec(&external).unwrap()).unwrap();assert!(replace(&j,false).is_err());assert_eq!(current(&j.target).unwrap(),external);
        j.before=Value::Null;j.after=launcher_after(&j.before,j.launcher.as_ref().unwrap()).unwrap();fs::write(&j.target,serde_json::to_vec(&j.after).unwrap()).unwrap();assert!(initial(&j).is_err(),"an existing list cannot authorize a forged create/restore deletion");
    }
    #[test]fn launcher_proof_rejects_receipt_build_manifest_and_pending_mutations(){
        let f=Fixture::new();let j=f.launcher();let engine=&j.launcher.as_ref().unwrap()[0];
        verify_launcher_proof(j.launcher.as_ref().unwrap()).unwrap();
        let receipt=engine.root.join(".egstore/um-install.json");let original=fs::read(&receipt).unwrap();let mut altered:Value=serde_json::from_slice(&original).unwrap();altered["build"]="5.8.4-evil".into();fs::write(&receipt,altered.to_string()).unwrap();assert!(verify_launcher_proof(j.launcher.as_ref().unwrap()).is_err());fs::write(&receipt,original).unwrap();
        fs::write(engine.root.join(".um-engine-update.json"),b"pending").unwrap();assert!(verify_launcher_proof(j.launcher.as_ref().unwrap()).is_err());fs::remove_file(engine.root.join(".um-engine-update.json")).unwrap();
        let version=engine.root.join("Engine/Build/Build.version");let original=fs::read(&version).unwrap();fs::write(&version,br#"{"MajorVersion":5,"MinorVersion":8,"PatchVersion":4,"Changelist":43}"#).unwrap();assert!(verify_launcher_proof(j.launcher.as_ref().unwrap()).is_err());fs::write(version,original).unwrap();
        fs::write(&engine.manifest,b"corrupt").unwrap();assert!(replace(&j,false).is_err());assert!(!j.target.exists());
    }
    #[cfg(windows)]
    #[test]fn rejects_network_and_device_paths_before_any_filesystem_probe(){
        for p in [r"\\server\share\job.json",r"\\?\UNC\server\share\job.json",r"\\.\C:\job.json",r"\\?\GLOBALROOT\Device\HarddiskVolume1\job.json",r"C:relative\job.json",r"C:\parent\..\job.json",r"\\?\C:\parent\..\job.json"] {assert!(local_absolute(Path::new(p)).is_err(),"{p}");}
        assert!(local_absolute(Path::new(r"X:\Fixture\job.json")).is_ok());
        assert!(local_absolute(Path::new(r"\\?\X:\Fixture\job.json")).is_ok());
    }
    #[test]fn mapped_remote_and_unavailable_drives_are_rejected(){for kind in [0,1,4,7,u32::MAX]{assert!(!allowed_drive_kind(kind));}for kind in [2,3,5,6]{assert!(allowed_drive_kind(kind));}}
    #[test]fn ancestor_rejection_precedes_any_descendant_probe(){
        let blocked=std::env::temp_dir().join("Fixture").join("observed-reparse");let leaf=blocked.join("must-not-contact/leaf");let mut seen=Vec::new();
        let result=walk_ancestors(&leaf,|part|{assert!(!part.starts_with(&blocked)||part==blocked);seen.push(part.to_path_buf());if part==blocked{Err("observed reparse".into())}else{Ok(true)}});
        assert!(result.is_err());assert_eq!(seen.last(),Some(&blocked));
        seen.clear();walk_ancestors(&leaf,|part|{seen.push(part.to_path_buf());Ok(part!=blocked)}).unwrap();assert_eq!(seen.last(),Some(&blocked));
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{mem::{size_of,zeroed}, os::windows::{ffi::OsStrExt,io::{AsRawHandle,FromRawHandle,OwnedHandle}}};
    use windows_sys::Win32::{Foundation::*, Security::{*,Authorization::*}, Storage::FileSystem::*, System::{Com::{CoTaskMemFree,CoInitializeEx,CoUninitialize,COINIT_APARTMENTTHREADED},IO::OVERLAPPED,Threading::*},UI::{Shell::*,WindowsAndMessaging::SW_HIDE}};
    fn wide(value: &std::ffi::OsStr)->Vec<u16>{value.encode_wide().chain(Some(0)).collect()}
    pub(super) fn local_drive(path:&Path)->Result<(),String>{use std::path::Prefix;let letter=match path.components().next(){Some(Component::Prefix(p))=>match p.kind(){Prefix::Disk(d)|Prefix::VerbatimDisk(d)=>d,_=>return Err(error("local disk path required"))},_=>return Err(error("drive letter required"))};let root=[letter as u16,b':' as u16,b'\\' as u16,0];let kind=unsafe{GetDriveTypeW(root.as_ptr())};if allowed_drive_kind(kind){Ok(())}else{Err(error("remote or unavailable drive is not permitted"))}}
    fn fail()->String{error(std::io::Error::last_os_error())}
    pub(super) struct Process {handle:OwnedHandle,pub id:u32}
    impl Process {
        pub fn open(id:u32)->Result<Self,String>{let h=unsafe{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|PROCESS_SYNCHRONIZE,0,id)};if h.is_null(){Err(fail())}else{Ok(Self{handle:unsafe{OwnedHandle::from_raw_handle(h)},id})}}
        fn raw(&self)->HANDLE{self.handle.as_raw_handle()}
        pub fn identity(&self)->Result<(u64,PathBuf),String>{unsafe{let(mut c,mut e,mut k,mut u)=(zeroed(),zeroed(),zeroed(),zeroed());if GetProcessTimes(self.raw(),&mut c,&mut e,&mut k,&mut u)==0{return Err(fail())}let mut b=vec![0u16;32768];let mut n=b.len() as u32;if QueryFullProcessImageNameW(self.raw(),0,b.as_mut_ptr(),&mut n)==0{return Err(fail())}let path=PathBuf::from(String::from_utf16_lossy(&b[..n as usize]));absolute(&path)?;Ok((((c.dwHighDateTime as u64)<<32)|c.dwLowDateTime as u64,fs::canonicalize(path).map_err(error)?))}}
        pub fn sid(&self)->Result<String,String>{unsafe{let mut t=std::ptr::null_mut();if OpenProcessToken(self.raw(),TOKEN_QUERY,&mut t)==0{return Err(fail())}let token=OwnedHandle::from_raw_handle(t);let mut n=0;GetTokenInformation(token.as_raw_handle(),TokenUser,std::ptr::null_mut(),0,&mut n);let mut b=vec![0usize;(n as usize+size_of::<usize>()-1)/size_of::<usize>()];if GetTokenInformation(token.as_raw_handle(),TokenUser,b.as_mut_ptr().cast(),n,&mut n)==0{return Err(fail())}sid_string((*(b.as_ptr().cast::<TOKEN_USER>())).User.Sid)}}
        pub fn alive(&self)->Result<bool,String>{match unsafe{WaitForSingleObject(self.raw(),0)}{WAIT_TIMEOUT=>Ok(true),WAIT_OBJECT_0=>Ok(false),_=>Err(fail())}}
        pub fn wait(&self,time:Duration)->Result<(),String>{if unsafe{WaitForSingleObject(self.raw(),time.as_millis().min(u32::MAX as u128) as u32)}==WAIT_OBJECT_0{Ok(())}else{Err(fail())}}
        pub fn terminate(&self)->Result<(),String>{if unsafe{TerminateProcess(self.raw(),1)}==0{return Err(fail())}self.wait(Duration::from_secs(5))}
    }
    pub(super) fn elevated()->Result<bool,String>{unsafe{let mut h=std::ptr::null_mut();if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut h)==0{return Err(fail())}let token=OwnedHandle::from_raw_handle(h);let mut v:TOKEN_ELEVATION=zeroed();let mut n=0;if GetTokenInformation(token.as_raw_handle(),TokenElevation,(&mut v as *mut TOKEN_ELEVATION).cast(),size_of::<TOKEN_ELEVATION>() as u32,&mut n)==0{return Err(fail())}Ok(v.TokenIsElevated!=0)}}
    fn program_data()->Result<PathBuf,String>{unsafe{let mut p=std::ptr::null_mut();if SHGetKnownFolderPath(&FOLDERID_ProgramData,0,std::ptr::null_mut(),&mut p)<0{return Err(fail())}let mut n=0;while *p.add(n)!=0{n+=1;}let root=PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(p,n)));CoTaskMemFree(p.cast());absolute(&root)?;Ok(root)}}
    pub(super) fn epic_directory()->Result<PathBuf,String>{let path=program_data()?.join("Epic/EpicGamesLauncher/Data/Manifests");absolute(&path)?;Ok(path)}
    pub(super) fn launcher_path()->Result<PathBuf,String>{let path=program_data()?.join("Epic/UnrealEngineLauncher/LauncherInstalled.dat");absolute(&path)?;Ok(path)}
    unsafe fn sid_string(sid:PSID)->Result<String,String>{let mut p=std::ptr::null_mut();if ConvertSidToStringSidW(sid,&mut p)==0{return Err(fail())}let mut n=0;while *p.add(n)!=0{n+=1;}let value=String::from_utf16_lossy(std::slice::from_raw_parts(p,n));LocalFree(p.cast());Ok(value)}
    pub(super) fn secure_directory(path:&Path,sid:&str)->Result<(),String>{unsafe{let s=wide(std::ffi::OsStr::new(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;BA)(A;OICI;FA;;;SY)")));let mut sd=std::ptr::null_mut();if ConvertStringSecurityDescriptorToSecurityDescriptorW(s.as_ptr(),1,&mut sd,std::ptr::null_mut())==0{return Err(fail())}let ok=SetFileSecurityW(wide(path.as_os_str()).as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd);LocalFree(sd);if ok==0{Err(fail())}else{Ok(())}}}
    pub(super) fn verify_directory(path:&Path,sid:&str)->Result<(),String>{unsafe{let mut owner=std::ptr::null_mut();let mut dacl=std::ptr::null_mut();let mut sd=std::ptr::null_mut();let code=GetNamedSecurityInfoW(wide(path.as_os_str()).as_ptr(),SE_FILE_OBJECT,OWNER_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION,&mut owner,std::ptr::null_mut(),&mut dacl,std::ptr::null_mut(),&mut sd);if code!=0{return Err(error(format!("private directory security {code}")))}let result=(||{if sid_string(owner)?!=sid || dacl.is_null() || (*dacl).AceCount!=3{return Err(error("untrusted private directory"))}let mut seen=std::collections::BTreeSet::new();for i in 0..3{let mut a=std::ptr::null_mut();if GetAce(dacl,i,&mut a)==0{return Err(fail())}let ace=&*(a.cast::<ACCESS_ALLOWED_ACE>());if ace.Header.AceType!=0 || ace.Mask!=FILE_ALL_ACCESS{return Err(error("unexpected private directory ACE"))}let s=sid_string((&ace.SidStart as *const u32).cast_mut().cast())?;if ![sid,"S-1-5-32-544","S-1-5-18"].contains(&s.as_str()) || !seen.insert(s){return Err(error("unexpected private directory principal"))}}Ok(())})();LocalFree(sd);result}}
    pub(super) fn one_link(file:&File)->Result<(),String>{unsafe{let mut info:BY_HANDLE_FILE_INFORMATION=zeroed();if GetFileInformationByHandle(file.as_raw_handle(),&mut info)==0{return Err(fail())}if info.nNumberOfLinks!=1{return Err(error("hard-linked broker input"))}Ok(())}}
    pub(super) fn verify_private_file(path:&Path,sid:&str)->Result<(),String>{verify_directory(path,sid)}
    #[cfg(any(test,feature="engine-test-cli"))]
    pub(super) fn dacl(path:&Path)->Result<Value,String>{unsafe{
        let mut acl=std::ptr::null_mut();let mut sd=std::ptr::null_mut();
        let code=GetNamedSecurityInfoW(wide(path.as_os_str()).as_ptr(),SE_FILE_OBJECT,DACL_SECURITY_INFORMATION,std::ptr::null_mut(),std::ptr::null_mut(),&mut acl,std::ptr::null_mut(),&mut sd);
        if code!=0{return Err(error(format!("DACL query {code}")))}
        let result=(||{let mut control=0;let mut revision=0;if GetSecurityDescriptorControl(sd,&mut control,&mut revision)==0{return Err(fail())}if acl.is_null(){return Err(error("missing DACL"))}let mut aces=Vec::new();for i in 0..(*acl).AceCount as u32{let mut p=std::ptr::null_mut();if GetAce(acl,i,&mut p)==0{return Err(fail())}let header=&*p.cast::<ACE_HEADER>();let mut bytes=std::slice::from_raw_parts(p.cast::<u8>(),header.AceSize as usize).to_vec();if bytes.len()<4{return Err(error("invalid ACE"))}/* ReplaceFile may recalculate only inherited provenance; masks, types, SID, other flags and order remain checked. */bytes[1]&=!0x10;aces.push(bytes.iter().map(|b|format!("{b:02x}")).collect::<String>());}Ok(serde_json::json!({"protected":control&SE_DACL_PROTECTED!=0,"aces":aces}))})();LocalFree(sd);result
    }}
    #[cfg(feature="engine-test-cli")]
    pub(super) fn protect_probe(path:&Path)->Result<(),String>{unsafe{let sid=Process::open(std::process::id())?.sid()?;let s=wide(std::ffi::OsStr::new(&format!("D:P(A;;WD;;;{sid})(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;BU)")));let mut sd=std::ptr::null_mut();if ConvertStringSecurityDescriptorToSecurityDescriptorW(s.as_ptr(),1,&mut sd,std::ptr::null_mut())==0{return Err(fail())}let ok=SetFileSecurityW(wide(path.as_os_str()).as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd);LocalFree(sd);if ok==0{Err(fail())}else{Ok(())}}}
    #[cfg(feature="engine-test-cli")]
    pub(super) fn delete_denied(path:&Path)->Result<bool,String>{use std::os::windows::fs::OpenOptionsExt;match OpenOptions::new().access_mode(DELETE).share_mode(7).open(path){Ok(_)=>Ok(false),Err(e) if e.raw_os_error()==Some(5)=>Ok(true),Err(e)=>Err(error(e))}}
    pub(super) fn launch(exe:&Path,job:&Path)->Result<Process,String>{unsafe{
        struct Com(bool);impl Drop for Com{fn drop(&mut self){if self.0{unsafe{CoUninitialize();}}}}
        let _com=Com(CoInitializeEx(std::ptr::null(),COINIT_APARTMENTTHREADED as u32)>=0);
        let verb=wide(std::ffi::OsStr::new("runas"));let file=wide(exe.as_os_str());let args=wide(std::ffi::OsStr::new(&format!("--epic-registration-broker \"{}\"",job.display())));
        let mut info:SHELLEXECUTEINFOW=zeroed();info.cbSize=size_of::<SHELLEXECUTEINFOW>() as u32;info.fMask=SEE_MASK_NOCLOSEPROCESS|SEE_MASK_NOASYNC;info.lpVerb=verb.as_ptr();info.lpFile=file.as_ptr();info.lpParameters=args.as_ptr();info.nShow=SW_HIDE;
        if ShellExecuteExW(&mut info)==0{return Err(if GetLastError()==ERROR_CANCELLED{CANCELLED.into()}else{fail()})}if info.hProcess.is_null(){return Err(error("missing broker process handle"))}let id=GetProcessId(info.hProcess);Ok(Process{handle:OwnedHandle::from_raw_handle(info.hProcess),id})
    }}
    pub(super) fn probe(target:&Path)->Result<(),String>{
        let parent=target.parent().ok_or_else(||error("target parent"))?;absolute(parent)?;
        if !parent.is_dir(){return Err(error("Epic manifests directory is unavailable"));}
        if target.exists(){
            let f=input(target)?;if f.metadata().map_err(error)?.permissions().readonly(){return Err(error("Epic record is read-only"));}drop(f);
            // The old target needs DELETE and data/synchronization rights;
            // WRITE_DAC is required on our new owned replacement, not on it.
            // A writable sibling
            // alone does not prove these rights on a protected existing item.
            use std::os::windows::fs::OpenOptionsExt;
            let mut rights=OpenOptions::new();
            rights.access_mode(DELETE|FILE_GENERIC_READ|FILE_GENERIC_WRITE).share_mode(7);
            let _access=rights.open(target).map_err(error)?;
        }
        let a=parent.join(format!(".um-probe-{}.tmp",uuid::Uuid::new_v4()));let b=a.with_extension("moved");write_new(&a,b"Epic metadata access probe")?;
        let r=move_new(&a,&b).and_then(|_|fs::remove_file(&b).map_err(error));if r.is_err(){let _=fs::remove_file(&a);let _=fs::remove_file(&b);}r
    }
    pub(super) fn move_new(from:&Path,to:&Path)->Result<(),String>{if unsafe{MoveFileExW(wide(from.as_os_str()).as_ptr(),wide(to.as_os_str()).as_ptr(),0)}==0{Err(fail())}else{Ok(())}}
    pub(super) fn replace(target:&Path,temp:&Path,backup:&Path,exists:bool)->Result<(),String>{if !exists{return move_new(temp,target)}if unsafe{ReplaceFileW(wide(target.as_os_str()).as_ptr(),wide(temp.as_os_str()).as_ptr(),wide(backup.as_os_str()).as_ptr(),0,std::ptr::null(),std::ptr::null())}==0{Err(fail())}else{Ok(())}}
    pub(super) struct FileLock{handle:HANDLE,overlapped:OVERLAPPED}
    impl FileLock{pub fn new(file:&File,exclusive:bool)->Result<Self,String>{let mut value=Self{handle:file.as_raw_handle(),overlapped:unsafe{zeroed()}};let start=Instant::now();loop{if unsafe{LockFileEx(value.handle,LOCKFILE_FAIL_IMMEDIATELY|if exclusive{LOCKFILE_EXCLUSIVE_LOCK}else{0},0,IPC_LIMIT as u32,0,&mut value.overlapped)}!=0{return Ok(value)}if unsafe{GetLastError()}!=ERROR_LOCK_VIOLATION || start.elapsed()>Duration::from_secs(1){return Err(fail())}std::thread::sleep(Duration::from_millis(10));}}}
    impl Drop for FileLock{fn drop(&mut self){unsafe{UnlockFileEx(self.handle,0,IPC_LIMIT as u32,0,&mut self.overlapped);}}}
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub struct Process{pub id:u32}
    pub fn local_drive(_:&Path)->Result<(),String>{Ok(())}
    impl Process{pub fn open(_:u32)->Result<Self,String>{Err(error("Windows required"))}pub fn identity(&self)->Result<(u64,PathBuf),String>{Err(error("Windows required"))}pub fn sid(&self)->Result<String,String>{Err(error("Windows required"))}pub fn alive(&self)->Result<bool,String>{Ok(false)}pub fn wait(&self,_:Duration)->Result<(),String>{Ok(())}pub fn terminate(&self)->Result<(),String>{Ok(())}}
    pub fn elevated()->Result<bool,String>{Ok(false)}pub fn epic_directory()->Result<PathBuf,String>{Err(error("Windows required"))}pub fn launcher_path()->Result<PathBuf,String>{Err(error("Windows required"))}pub fn secure_directory(_:&Path,_:&str)->Result<(),String>{Err(error("Windows required"))}pub fn verify_directory(_:&Path,_:&str)->Result<(),String>{Err(error("Windows required"))}pub fn verify_private_file(_:&Path,_:&str)->Result<(),String>{Err(error("Windows required"))}pub fn one_link(_:&File)->Result<(),String>{Ok(())}pub fn launch(_:&Path,_:&Path)->Result<Process,String>{Err(error("Windows required"))}pub fn probe(_:&Path)->Result<(),String>{Err(error("Windows required"))}pub fn move_new(from:&Path,to:&Path)->Result<(),String>{fs::rename(from,to).map_err(error)}pub fn replace(target:&Path,temp:&Path,backup:&Path,exists:bool)->Result<(),String>{if exists{fs::rename(target,backup).map_err(error)?;}fs::rename(temp,target).map_err(error)}
    pub struct FileLock;impl FileLock{pub fn new(_:&File,_:bool)->Result<Self,String>{Ok(Self)}}
}

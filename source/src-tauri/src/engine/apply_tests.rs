// Native apply safety/performance fixtures only. No app, registry, real engine,
// account, session, network or existing task data is used.
use super::{Options, Snapshot};
use super::super::*;
use crate::engine::{Plan, Preview, Component, FileManifestList};
use egs_api::api::types::download_manifest::FileChunkPart;
use std::{cell::Cell, io::{Read, Write}, sync::{Arc, Condvar}, time::{Duration, Instant}};

const FROZEN_BASELINE_COMMIT:&str="dce2e78";
const FROZEN_INSTALL_SOURCE_SHA256:&str="a6915e3fd33d19c4de7e51266362396a7e348e53183d9a34515f7336622f34d8";

pub(super) fn permission_baseline(plan:&Plan,stage:&Path,ops:&[Operation])->Result<Option<String>,String>{
    frozen_dce2::original_commit_checkpointed(plan,stage,ops,None,None,None,|_,_,_|{})
}

mod frozen_dce2 {
use super::*;
    thread_local! {static READ_BYTES:Cell<u64>=const{Cell::new(0)};}
    pub(super) fn reset_bytes(){READ_BYTES.with(|n|n.set(0));}
    pub(super) fn read_bytes()->u64{READ_BYTES.with(Cell::get)}
    // Original shared vault::hash_file behavior: path guard/open/SHA1/64KiB,
    // with only a thread-local byte counter for the serial-update benchmark.
    fn frozen_io_error(_:std::io::Error)->String{"Ошибка файловой системы. Проверьте свободное место и доступ к каталогу".into()}
    fn hash_file(path:&Path)->Result<String,String>{
        no_links(path)?;let mut file=fs::File::open(path).map_err(frozen_io_error)?;
        let mut hash=Sha1::new();let mut buffer=[0u8;65536];
        loop{let n=file.read(&mut buffer).map_err(frozen_io_error)?;if n==0{break;}hash.update(&buffer[..n]);READ_BYTES.with(|b|b.set(b.get()+n as u64));}
        Ok(format!("{:x}",hash.finalize()))
    }
fn original_verify_current(root: &Path, op: &Operation) -> Result<(), String> {
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

pub(super) fn original_commit_checkpointed(plan:&Plan,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,preflight_warning:Option<String>,mut progress:impl FnMut(&str,u64,u64))->Result<Option<String>,String>{
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
        if original_bulk_layout(&root,stage,&ops,paused,suspend,&mut progress)? {
            original_verify_bulk(&root,stage,&ops,paused,suspend,&mut progress)?;
            pause_checkpoint(paused,||progress("Проверяем файлы перед установкой",ops.len() as u64,ops.len() as u64));
            crate::engine::resume::check_suspend(suspend)?;
            idle(&root)?;
            if !empty_installation(&root)? {return Err("В папке появились файлы во время установки".into());}
            
            for entry in fs::read_dir(stage.join("files")).map_err(io)? {
                crate::engine::resume::check_suspend(suspend)?;
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
            crate::engine::resume::check_suspend(suspend)?;
            if idle_checkpoint_due(last_idle) {
                idle(&root)?;
                last_idle = Instant::now();
            }
            if restoring && original_operation_applied(&root,stage,op)? {
                if last_progress.elapsed()>=Duration::from_millis(250)||i+1==ops.len(){progress("Устанавливаем движок",(i+1) as u64,ops.len() as u64);last_progress=Instant::now();}continue;
            }
            let rel = relative(&op.name)?;
            let target = root.join(&rel);
            let source = stage.join("files").join(&rel);
            let backup = stage.join("originals").join(&rel);
            let backup_present=restoring&&backup.exists();
            if !backup_present{original_verify_current(&root,op)?;}else{if op.old_hash.as_ref()!=Some(&hash_file(&backup)?){return Err("Резервный файл изменён".into());}no_links(&target)?;if target.exists(){return Err("При продолжении обнаружены внешние изменения".into());}}
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
        if error==crate::engine::resume::SUSPENDED{return Err(error);}
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

fn original_operation_applied(root:&Path,stage:&Path,op:&Operation)->Result<bool,String>{
    let rel=relative(&op.name)?;let target=root.join(&rel);let source=stage.join("files").join(&rel);let backup=stage.join("originals").join(&rel);
    no_links(&target)?;no_links(&source)?;no_links(&backup)?;
    if let Some(old)=&op.old_hash{if !backup.exists(){return Ok(false);}if hash_file(&backup)?!=*old{return Err("Резервный файл изменён".into());}
        return match &op.new_hash{None=>if target.exists(){Err("При продолжении обнаружены внешние изменения".into())}else{Ok(true)},Some(new)=>if target.exists(){if hash_file(&target)?==*new{Ok(true)}else{Err("При продолжении обнаружены внешние изменения".into())}}else{Ok(false)}};
    }
    if target.exists(){if op.new_hash.as_ref()==Some(&hash_file(&target)?)&&!source.exists(){Ok(true)}else{Err("При продолжении обнаружены внешние изменения".into())}}else{Ok(false)}
}

fn original_bulk_layout(root:&Path,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,progress:&mut impl FnMut(&str,u64,u64))->Result<bool,String>{
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
            crate::engine::resume::check_suspend(suspend)?;
            if last.elapsed()>=Duration::from_millis(250){progress("Проверяем файлы перед установкой",0,ops.len() as u64);last=Instant::now();}
            let path=e.map_err(io)?.path();let meta=fs::symlink_metadata(&path).map_err(io)?;
            #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("Ссылки в рабочей папке не поддерживаются".into());}}
            if meta.file_type().is_symlink(){return Err("Ссылки в рабочей папке не поддерживаются".into());}
            if meta.is_dir(){pending.push(path);}else if meta.is_file(){found.insert(path.strip_prefix(&base).map_err(|_|"Путь вне рабочей папки")?.to_path_buf());}else{return Ok(false);}
        }
    }
    Ok(found==expected)
}

fn original_verify_bulk(root:&Path,stage:&Path,ops:&[Operation],paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,progress:&mut impl FnMut(&str,u64,u64))->Result<(),String>{
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
            if let Err(e)=crate::engine::resume::check_suspend(suspend){failed.store(true,Ordering::SeqCst);return Err(e);}
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
}

#[path="apply_failure_tests.rs"]
mod failure_matrix;

struct Fixture { base: PathBuf, id: String, marker_ready: bool }
impl Fixture {
    fn new() -> Self {
        let parent = std::env::var_os("UM_ENGINE_APPLY_FIXTURE_ROOT")
            .map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute(), "Fixture root must be absolute");
        no_links(&parent).unwrap();
        fs::create_dir_all(&parent).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let base = parent.join(format!("um-engine-apply-{id}"));
        fs::create_dir(&base).unwrap();
        let mut fixture = Self { base, id, marker_ready: false };
        let mut marker = fs::OpenOptions::new().write(true).create_new(true)
            .open(fixture.base.join("fixture-owner.txt")).unwrap();
        marker.write_all(fixture.id.as_bytes()).unwrap();
        fixture.marker_ready = true;
        fs::create_dir(fixture.base.join("engine")).unwrap();
        fs::create_dir_all(fixture.base.join("stage/files")).unwrap();
        fixture
    }
    fn root(&self) -> PathBuf { self.base.join("engine") }
    fn stage(&self) -> PathBuf { self.base.join("stage") }
    fn stage_write(&self,name:&str,data:&[u8]) {let p=self.stage().join("files").join(relative(name).unwrap());fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,data).unwrap();}
    fn write(&self, name: &str, data: &[u8]) {
        let p = self.root().join(relative(name).unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
    fn plan(&self) -> Plan {
        Plan {
            resume_stage: None, catalog_item_id: None, remove_unselected: false,
            source_base: String::new(), raw: b"synthetic apply manifest".to_vec(), files: BTreeMap::new(),
            old: BTreeMap::new(), item: None, chunk_sizes: BTreeMap::new(), tags: BTreeMap::new(),
            info: Preview {
                id: self.id.clone(), app_name: "UE_SYNTHETIC".into(), build: "synthetic-1".into(),
                path: self.root().to_string_lossy().into_owned(), update: true,
                components: Vec::<Component>::new(), bytes: 0, download_bytes: 0,
                changed: 0, added: 0, removed: 0, required_bytes: 0, free_bytes: 0, conflicts: vec![],
            },
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Failure/panic cleanup is confined to this freshly-created UUID, never
        // to the caller's fixture parent or any engine/user-data location.
        let result = (|| -> Result<(), String> {
            if self.base.file_name().and_then(|n| n.to_str()) != Some(format!("um-engine-apply-{}",self.id).as_str()) {
                return Err("Fixture ownership mismatch".into());
            }
            no_links(&self.base)?;
            if !self.marker_ready {
                // Constructor failed before the complete marker: accept only
                // our newly-created empty directory or its sole partial marker.
                let entries = fs::read_dir(&self.base).map_err(|e|e.to_string())?
                    .collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                if entries.len() == 1 && entries[0].file_name() == "fixture-owner.txt" {
                    let marker = entries[0].path(); no_links(&marker)?;
                    let partial = fs::read(&marker).map_err(|e|e.to_string())?;
                    if !self.id.as_bytes().starts_with(&partial) { return Err("Unknown partial fixture marker".into()); }
                    fs::remove_file(marker).map_err(|e|e.to_string())?;
                } else if !entries.is_empty() { return Err("Unknown incomplete fixture contents".into()); }
                return fs::remove_dir(&self.base).map_err(|e|e.to_string());
            }
            if fs::read(self.base.join("fixture-owner.txt")).map_err(|e|e.to_string())? != self.id.as_bytes() {
                return Err("Fixture marker changed".into());
            }
            let mut pending = vec![self.base.clone()]; let mut count = 0;
            while let Some(dir) = pending.pop() {
                for entry in fs::read_dir(dir).map_err(|e|e.to_string())? {
                    let p = entry.map_err(|e|e.to_string())?.path();
                    let m = fs::symlink_metadata(&p).map_err(|e|e.to_string())?;
                    if m.file_type().is_symlink() { return Err("Linked fixture entry".into()); }
                    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; if m.file_attributes() & 0x400 != 0 { return Err("Reparse fixture entry".into()); } }
                    count += 1; if count > 30_000 { return Err("Fixture inventory bound".into()); }
                    if m.is_dir() { pending.push(p); } else if !m.is_file() { return Err("Unknown fixture entry".into()); }
                }
            }
            fs::remove_dir_all(&self.base).map_err(|e|e.to_string())
        })();
        if let Err(error) = result { eprintln!("Owned apply fixture retained: {error}"); }
    }
}

fn entry(name:&str,data:&[u8])->FileManifestList {
    FileManifestList{filename:name.into(),file_hash:digest(data),file_chunk_parts:vec![FileChunkPart{guid:String::new(),link:None,offset:0,size:data.len()as u128}]}
}
fn update_case(tiny_count:usize,large_count:usize,large_size:usize)->(Fixture,Plan,Vec<Operation>) {
    let f=Fixture::new();let mut plan=f.plan();let mut ops=vec![];
    for(group,count,size)in [("Small",tiny_count,4096),("Large",large_count,large_size)] {
        let old=vec![41u8;size];let new=vec![73u8;size];
        for i in 0..count {
            let name=format!("Engine/{group}/{i:05}.bin");f.write(&name,&old);f.stage_write(&name,&new);
            plan.old.insert(name.clone(),entry(&name,&old));plan.files.insert(name.clone(),entry(&name,&new));
            ops.push(Operation{name,old_hash:Some(digest(&old)),new_hash:Some(digest(&new))});
        }
    }
    ops.sort_by(|a,b|a.name.cmp(&b.name));
    f.write("Engine/Plugins/UserPlugin/work",b"user work must survive");
    (f,plan,ops)
}
fn settings(workers:usize,buffer_bytes:usize)->Options {
    Options{workers,buffer_bytes,interval:Duration::ZERO,..Default::default()}
}
fn run(plan:&Plan,stage:&Path,ops:&[Operation],options:Options,progress:impl FnMut(Snapshot))->Result<Option<String>,String> {
    super::super::commit_detailed_with_options(plan,stage,ops,None,None,None,options,progress)
}
fn assert_final(f:&Fixture,ops:&[Operation]) {
    for op in ops {assert_eq!(hash_file(&f.root().join(&op.name)).unwrap(),*op.new_hash.as_ref().unwrap());}
    assert_eq!(fs::read(f.root().join("Engine/Plugins/UserPlugin/work")).unwrap(),b"user work must survive");
}

#[derive(Default)]
struct GateState {claimed:usize,arrived:usize,bytes:u64,released:bool,timed_out:bool}
struct ReadGate {needed:usize,state:Mutex<GateState>,changed:Condvar}
impl ReadGate {
    fn new(needed:usize)->Arc<Self>{Arc::new(Self{needed,state:Mutex::new(GateState::default()),changed:Condvar::new()})}
    fn hook(&self,_:&Path,bytes:usize) {
        let mut state=self.state.lock().unwrap();if state.claimed>=self.needed{return;}
        state.claimed+=1;state.arrived+=1;state.bytes+=bytes as u64;self.changed.notify_all();
        let deadline=Instant::now()+Duration::from_secs(10);
        while !state.released {
            let remaining=deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero(){state.timed_out=true;state.released=true;self.changed.notify_all();break;}
            state=self.changed.wait_timeout(state,remaining).unwrap().0;
        }
    }
    fn wait_ready(&self) {
        let mut state=self.state.lock().unwrap();let deadline=Instant::now()+Duration::from_secs(10);
        while state.arrived<self.needed && !state.timed_out {
            let remaining=deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(),"Workers never reached the deterministic read barrier");
            state=self.changed.wait_timeout(state,remaining).unwrap().0;
        }
        assert!(!state.timed_out);assert_eq!(state.arrived,self.needed);
    }
    fn release(&self){let mut state=self.state.lock().unwrap();state.released=true;self.changed.notify_all();}
    fn bytes(&self)->u64{self.state.lock().unwrap().bytes}
    fn assert_bounded(&self){assert!(!self.state.lock().unwrap().timed_out,"Read hook exceeded its deadlock-safety deadline");}
}
struct ReleaseGate(Arc<ReadGate>);
impl Drop for ReleaseGate {fn drop(&mut self){self.0.release();}}
fn gated_options(workers:usize,gate:&Arc<ReadGate>)->Options {
    let gate=gate.clone();Options{read_hook:Some(Arc::new(move|path,bytes|gate.hook(path,bytes))),..settings(workers,65536)}
}

#[test]
fn apply_checks_exact_bytes_and_advances_global_files_only_after_replacement() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(3,1,256*1024);let mut events=vec![];let mut saw_read=false;
        run(&plan,&f.stage(),&ops,settings(workers,65536),|s|{
            if s.current_file_stage.is_some_and(|v|matches!(v,"readOld"|"readNew"|"readBoth")){saw_read=true;assert!(s.done<s.total);}
            if s.phase=="Устанавливаем движок" && s.total>0 && s.done==s.total {assert_final(&f,&ops);}
            events.push(s);
        }).unwrap();
        assert!(saw_read);assert_final(&f,&ops);
        let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();
        assert_eq!(j.status,"complete");assert!(!f.root().join(".um-engine-update.json").exists());
        assert!(events.iter().all(|s|s.done<=s.total));
        assert!(events.iter().any(|s|s.current_file_stage==Some("replace")&&s.done<s.total));
    }
}

#[cfg(windows)]
#[test]
fn current_operation_read_handles_refuse_external_write_and_delete_until_commit() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(0,1,1024*1024);let gate=ReadGate::new(workers);let mut checked=false;let mut precommit_checked=false;
        run(&plan,&f.stage(),&ops,gated_options(workers,&gate),|s|{
            if s.read_bytes>0 && !checked {
                let _release=ReleaseGate(gate.clone());gate.wait_ready();checked=true;
                let mut locked=vec![f.root().join(&ops[0].name)];
                if workers==2{locked.push(f.stage().join("files").join(&ops[0].name));}
                for p in locked {
                    assert!(fs::OpenOptions::new().write(true).open(&p).is_err(),"Verified read must deny concurrent writers");
                    assert!(fs::rename(&p,p.with_extension("unexpected-move")).is_err(),"Verified read must deny path replacement/deletion");
                }
                assert_eq!(s.done,0);assert!(s.current_file_done.unwrap_or(0)>0);
            }
            if s.current_file_stage==Some("replace")&&s.done==0&&!precommit_checked {
                for p in [f.root().join(&ops[0].name),f.stage().join("files").join(&ops[0].name)] {
                    assert!(fs::OpenOptions::new().write(true).open(p).is_err(),"Both verified handles must remain held before mutation");
                }
                precommit_checked=true;
            }
        }).unwrap();
        gate.assert_bounded();assert!(checked&&precommit_checked);assert_final(&f,&ops);
    }
}

#[test]
fn mutations_to_a_later_operation_are_freshly_detected_and_prior_changes_roll_back() {
    for workers in [1,2] {for mutate_source in [false,true] {
        // Tiny current operations intentionally use one reader even when the
        // storage policy permits a paired large-file helper.
        let(f,plan,ops)=update_case(2,0,0);let gate=ReadGate::new(1);let mut changed=false;
        let later=if mutate_source{f.stage().join("files").join(&ops[1].name)}else{f.root().join(&ops[1].name)};
        let result=run(&plan,&f.stage(),&ops,gated_options(workers,&gate),|s|{
            if s.read_bytes>0 && !changed {let _release=ReleaseGate(gate.clone());gate.wait_ready();fs::write(&later,b"external edit after the transaction started").unwrap();changed=true;}
        });
        assert!(result.is_err());assert!(changed);gate.assert_bounded();
        assert_eq!(hash_file(&f.root().join(&ops[0].name)).unwrap(),*ops[0].old_hash.as_ref().unwrap());
        if mutate_source {assert_eq!(hash_file(&f.root().join(&ops[1].name)).unwrap(),*ops[1].old_hash.as_ref().unwrap());}
        else {assert_eq!(fs::read(&later).unwrap(),b"external edit after the transaction started");}
        assert_eq!(fs::read(f.root().join("Engine/Plugins/UserPlugin/work")).unwrap(),b"user work must survive");
        let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(j.status,"rolledBack");
    }}
}

#[test]
fn suspend_during_a_paused_large_file_keeps_the_091_journal_and_releases_all_handles() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(0,1,2*1024*1024);let gate=ReadGate::new(if cfg!(windows){workers}else{1});
        let paused=Arc::new(AtomicBool::new(false));let suspend=Arc::new(AtomicBool::new(false));
        let mut paused_at=None;let mut paused_observed=false;
        // A watchdog only prevents a broken pause/suspend implementation from
        // hanging the suite. It is not a performance/timing pass criterion.
        let(tx,rx)=std::sync::mpsc::channel();let wp=paused.clone();let ws=suspend.clone();
        let expired=Arc::new(AtomicBool::new(false));let we=expired.clone();
        let watchdog=std::thread::spawn(move||{if rx.recv_timeout(Duration::from_secs(10)).is_err(){we.store(true,Ordering::SeqCst);ws.store(true,Ordering::SeqCst);wp.store(false,Ordering::SeqCst);}});
        let result=super::super::commit_detailed_with_options(&plan,&f.stage(),&ops,Some(&paused),Some(&suspend),None,gated_options(workers,&gate),|s|{
            if s.read_bytes>0 && paused_at.is_none() {
                let _release=ReleaseGate(gate.clone());gate.wait_ready();paused.store(true,Ordering::SeqCst);paused_at=Some(gate.bytes());
            }else if let Some(bytes)=paused_at {
                if paused.load(Ordering::SeqCst) && !suspend.load(Ordering::SeqCst) {
                    assert_eq!(s.read_bytes,bytes,"Paused hashing must not consume another buffer");assert_eq!(s.done,0);
                    paused_observed=true;suspend.store(true,Ordering::SeqCst);
                }
            }
        });
        let _=tx.send(());watchdog.join().unwrap();gate.assert_bounded();
        assert!(!expired.load(Ordering::SeqCst));assert!(paused_observed);assert_eq!(result.err().unwrap(),crate::engine::resume::SUSPENDED);
        assert_eq!(hash_file(&f.root().join(&ops[0].name)).unwrap(),*ops[0].old_hash.as_ref().unwrap());
        assert_eq!(hash_file(&f.stage().join("files").join(&ops[0].name)).unwrap(),*ops[0].new_hash.as_ref().unwrap());
        let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(j.status,"applying");
        assert!(f.root().join(".um-engine-update.json").exists());
        // A writable open after return proves no worker retained its guard.
        for p in [f.root().join(&ops[0].name),f.stage().join("files").join(&ops[0].name)] {drop(fs::OpenOptions::new().write(true).open(p).unwrap());}
    }
}

fn prepare_091_restart(f:&Fixture,plan:&Plan,ops:&[Operation]) {
    let stop=AtomicBool::new(true);
    let result=frozen_dce2::original_commit_checkpointed(plan,&f.stage(),ops,None,Some(&stop),None,|_,_,_|{});
    assert_eq!(result.err().unwrap(),crate::engine::resume::SUSPENDED);
    let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();
    assert_eq!(j.status,"applying");
}

#[test]
fn resume_accepts_a_real_091_journal_with_applied_and_backup_only_operations() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(3,0,0);prepare_091_restart(&f,&plan,&ops);
        for (index,op) in ops.iter().enumerate().take(2) {
            let target=f.root().join(&op.name);let backup=f.stage().join("originals").join(&op.name);
            fs::create_dir_all(backup.parent().unwrap()).unwrap();fs::rename(&target,&backup).unwrap();
            if index==0 {fs::rename(f.stage().join("files").join(&op.name),&target).unwrap();}
        }
        let applied_time=fs::metadata(f.root().join(&ops[0].name)).unwrap().modified().unwrap();
        run(&plan,&f.stage(),&ops,settings(workers,65536),|_|{}).unwrap();assert_final(&f,&ops);
        assert_eq!(fs::metadata(f.root().join(&ops[0].name)).unwrap().modified().unwrap(),applied_time);
        for op in &ops {assert_eq!(hash_file(&f.stage().join("originals").join(&op.name)).unwrap(),*op.old_hash.as_ref().unwrap());}
        let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(j.status,"complete");
    }
}

#[test]
fn worker_panic_returns_an_error_and_releases_verified_handles_without_mutation() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(0,1,256*1024);
        let old=f.root().join(&ops[0].name);let source=f.stage().join("files").join(&ops[0].name);
        let old_for_hook=old.clone();
        let options=Options{read_hook:Some(Arc::new(move|path,_|{if path==old_for_hook.as_path(){panic!("synthetic worker read panic");}})),..settings(workers,65536)};
        assert!(run(&plan,&f.stage(),&ops,options,|_|{}).is_err());
        assert_eq!(hash_file(&old).unwrap(),*ops[0].old_hash.as_ref().unwrap());
        assert_eq!(hash_file(&source).unwrap(),*ops[0].new_hash.as_ref().unwrap());
        drop(fs::OpenOptions::new().write(true).open(old).unwrap());
        drop(fs::OpenOptions::new().write(true).open(source).unwrap());
    }
}

#[test]
fn coordinator_callback_panic_stops_paused_readers_and_allows_journal_recovery() {
    for workers in [1,2] {
        let(f,plan,ops)=update_case(0,1,4*1024*1024);
        let gate=ReadGate::new(if cfg!(windows){workers}else{1});
        let paused=Arc::new(AtomicBool::new(false));let suspend=Arc::new(AtomicBool::new(false));
        let(tx,rx)=std::sync::mpsc::channel();let wp=paused.clone();let ws=suspend.clone();
        let expired=Arc::new(AtomicBool::new(false));let we=expired.clone();
        let watchdog=std::thread::spawn(move||{if rx.recv_timeout(Duration::from_secs(10)).is_err(){we.store(true,Ordering::SeqCst);ws.store(true,Ordering::SeqCst);wp.store(false,Ordering::SeqCst);}});
        let mut panicked_in_read=false;
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            super::super::commit_detailed_with_options(&plan,&f.stage(),&ops,Some(&paused),Some(&suspend),None,gated_options(workers,&gate),|s|{
                if s.read_bytes>0 {
                    let _release=ReleaseGate(gate.clone());gate.wait_ready();
                    assert_eq!(s.done,0);paused.store(true,Ordering::SeqCst);panicked_in_read=true;
                    panic!("synthetic coordinator callback panic with paused readers");
                }
            })
        }));
        let _=tx.send(());watchdog.join().unwrap();
        assert!(result.is_err()&&panicked_in_read);assert!(!expired.load(Ordering::SeqCst));gate.assert_bounded();
        let old=f.root().join(&ops[0].name);let source=f.stage().join("files").join(&ops[0].name);
        assert_eq!(hash_file(&old).unwrap(),*ops[0].old_hash.as_ref().unwrap());
        assert_eq!(hash_file(&source).unwrap(),*ops[0].new_hash.as_ref().unwrap());
        drop(fs::OpenOptions::new().write(true).open(old).unwrap());
        drop(fs::OpenOptions::new().write(true).open(source).unwrap());
        let j:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(j.status,"applying");
        run(&plan,&f.stage(),&ops,settings(workers,65536),|_|{}).unwrap();assert_final(&f,&ops);
    }
}

// Create a genuine 0.9.1 journal before changing any fixture paths. Half the
// operations then represent already-renamed files, and one has only its old
// backup, matching restart boundaries of the original serial transaction.
fn benchmark_restart(f:&Fixture,plan:&Plan,ops:&[Operation])->u64 {
    prepare_091_restart(f,plan,ops);
    let applied=ops.len()/2;let mut extra_baseline_read=0;
    for(index,op)in ops.iter().enumerate().take(applied+1) {
        let target=f.root().join(&op.name);let backup=f.stage().join("originals").join(&op.name);
        fs::create_dir_all(backup.parent().unwrap()).unwrap();fs::rename(&target,&backup).unwrap();
        if index<applied{fs::rename(f.stage().join("files").join(&op.name),&target).unwrap();}
        else{extra_baseline_read=fs::metadata(&backup).unwrap().len();}
    }
    extra_baseline_read
}

fn assert_complete_journal(f:&Fixture)->(usize,u64) {
    let journal:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();
    assert_eq!(journal.status,"complete");let mut bytes=0;
    for op in &journal.operations {
        if let Some(hash)=&op.old_hash {
            let path=f.stage().join("originals").join(&op.name);assert_eq!(hash_file(&path).unwrap(),*hash);bytes+=fs::metadata(path).unwrap().len();
        }
        if let Some(hash)=&op.new_hash {
            let path=f.root().join(&op.name);assert_eq!(hash_file(&path).unwrap(),*hash);bytes+=fs::metadata(path).unwrap().len();
        }else{assert!(!f.root().join(&op.name).exists());}
    }
    assert!(!f.root().join(".um-engine-update.json").exists());(journal.operations.len(),bytes)
}

#[test]
#[ignore = "Explicit owned-D synthetic transaction benchmark; no timing pass/fail or user-engine reads"]
fn benchmark_frozen_091_serial_against_locked_per_operation_apply() {
    // Recreated fixtures give each mode identical logical inputs. Setup, old
    // journal construction, final SHA verification and cleanup are NOT timed.
    // The OS cache is uncontrolled and warm; these numbers cannot predict a
    // user's installation drive or justify a drive-independent speed claim.
    let production_only=std::env::var("UM_ENGINE_APPLY_BENCH_PRODUCTION_ONLY").as_deref()==Ok("1");
    for profile in ["new-update","091-partial-resume"] {
        for round in 0..2 {
            let mut modes=if production_only{vec![(0,65536),(2,1048576)]}else{vec![(0,65536),(1,65536),(1,1048576),(2,65536),(2,1048576)]};
            if round==1{modes.reverse();}
            for(workers,buffer_bytes)in modes {
                let(f,plan,ops)=update_case(1024,4,4*1024*1024);
                let extra_baseline_read=if profile=="091-partial-resume"{benchmark_restart(&f,&plan,&ops)}else{0};
                let production_workers_hint=crate::engine::scan_storage::workers_hint(&f.root()).min(2);
                let mut reported_bytes=0;let mut final_logical=None;frozen_dce2::reset_bytes();
                let start=Instant::now();
                if workers==0 {
                    frozen_dce2::original_commit_checkpointed(&plan,&f.stage(),&ops,None,None,None,|phase,done,total|{
                        if phase=="Устанавливаем движок"{final_logical=Some((done,total));}
                    }).unwrap();
                }else{
                    // Production uses 250 ms, including every benchmark mode.
                    run(&plan,&f.stage(),&ops,Options{interval:Duration::from_millis(250),..settings(workers,buffer_bytes)},|s|{
                        reported_bytes=reported_bytes.max(s.read_bytes);
                        if s.phase=="Устанавливаем движок"{final_logical=Some((s.done,s.total));}
                    }).unwrap();
                }
                let elapsed=start.elapsed();
                let actual_read_bytes=if workers==0{frozen_dce2::read_bytes()}else{reported_bytes};
                assert_final(&f,&ops);let(operation_count,required_read_bytes)=assert_complete_journal(&f);
                assert_eq!(final_logical,Some((operation_count as u64,operation_count as u64)));
                // The original resume reads a backup-only old file twice. The
                // new core still freshly checks both required hashes once.
                assert_eq!(actual_read_bytes,required_read_bytes+if workers==0{extra_baseline_read}else{0});
                println!("ENGINE_APPLY_BENCH {}",serde_json::json!({
                    "schema":1,"profile":profile,"round":round,"modeSelection":if production_only{"production-only"}else{"all-options"},"algorithm":if workers==0{"frozen-091-serial"}else{"locked-per-operation"},
                    "baselineCommit":FROZEN_BASELINE_COMMIT,"baselineInstallSourceSha256":FROZEN_INSTALL_SOURCE_SHA256,
                    "workers":workers.max(1),"bufferBytes":buffer_bytes,"productionWorkersHint":production_workers_hint,
                    "fixtureId":f.id,"tinyFiles":1024,"tinyFileBytes":4096,"largeFiles":4,"largeFileBytes":4*1024*1024,
                    "operationsIncludingMetadata":operation_count,"actualHashReadBytes":actual_read_bytes,"requiredHashReadBytes":required_read_bytes,
                    "elapsedMs":elapsed.as_secs_f64()*1000.0,"logicalFilesPerSecond":operation_count as f64/elapsed.as_secs_f64().max(f64::EPSILON),
                    "actualHashReadBytesPerSecond":actual_read_bytes as f64/elapsed.as_secs_f64().max(f64::EPSILON),
                    "finalBytesAndBackupsVerified":true,"setupAndFinalVerificationTimed":false,
                    "cacheState":"Uncontrolled warm OS cache; no cache flush; recreated fixture per mode; second round reverses mode order",
                    "fixtureParentFromEnvironment":std::env::var_os("UM_ENGINE_APPLY_FIXTURE_ROOT").is_some(),"userEngineRead":false
                }));
            }
        }
    }
}


// Append to apply_tests.rs only after the currently-running benchmark has
// finished and the root explicitly opens the source window. This uses the
// already-frozen fixture/baseline helpers, not an alternate production path.
#[test]
#[ignore = "Explicit bounded large-pair synthetic benchmark; opt-in environment required"]
fn benchmark_large_pairs_against_frozen_091_apply() {
    assert_eq!(std::env::var("UM_ENGINE_APPLY_LARGE_BENCH").as_deref(),Ok("1"),"Explicit root-only large benchmark opt-in required");
    let production_only=std::env::var("UM_ENGINE_APPLY_BENCH_PRODUCTION_ONLY").as_deref()==Ok("1");
    for profile in ["new-update-large-pairs","091-partial-resume-large-pairs"] {
        for round in 0..2 {
            let mut modes=if production_only{vec![0,2]}else{vec![0,1,2]};if round==1{modes.reverse();}
            for workers in modes {
                let(f,plan,ops)=update_case(8,2,128*1024*1024);
                let extra_baseline_read=if profile.starts_with("091-"){benchmark_restart(&f,&plan,&ops)}else{0};
                let production_workers_hint=crate::engine::scan_storage::workers_hint(&f.root()).min(2);
                let mut reported_bytes=0;let mut final_logical=None;frozen_dce2::reset_bytes();
                let start=Instant::now();
                if workers==0 {
                    frozen_dce2::original_commit_checkpointed(&plan,&f.stage(),&ops,None,None,None,|phase,done,total|{
                        if phase=="Устанавливаем движок"{final_logical=Some((done,total));}
                    }).unwrap();
                }else{
                    run(&plan,&f.stage(),&ops,Options{interval:Duration::from_millis(250),..settings(workers,1048576)},|s|{
                        reported_bytes=reported_bytes.max(s.read_bytes);
                        if s.phase=="Устанавливаем движок"{final_logical=Some((s.done,s.total));}
                    }).unwrap();
                }
                let elapsed=start.elapsed();
                let actual_read_bytes=if workers==0{frozen_dce2::read_bytes()}else{reported_bytes};
                assert_final(&f,&ops);let(operation_count,required_read_bytes)=assert_complete_journal(&f);
                assert_eq!(final_logical,Some((operation_count as u64,operation_count as u64)));
                assert_eq!(actual_read_bytes,required_read_bytes+if workers==0{extra_baseline_read}else{0});
                println!("ENGINE_APPLY_LARGE_BENCH {}",serde_json::json!({
                    "schema":1,"profile":profile,"round":round,"modeSelection":if production_only{"production-only"}else{"all-options"},"algorithm":if workers==0{"frozen-091-serial"}else{"locked-per-operation"},
                    "baselineCommit":FROZEN_BASELINE_COMMIT,"baselineInstallSourceSha256":FROZEN_INSTALL_SOURCE_SHA256,
                    "workers":workers.max(1),"bufferBytes":if workers==0{65536}else{1048576},"productionWorkersHint":production_workers_hint,
                    "fixtureId":f.id,"tinyFiles":8,"tinyFileBytes":4096,"largeFiles":2,"largeFileBytes":128*1024*1024,
                    "operationsIncludingMetadata":operation_count,"actualHashReadBytes":actual_read_bytes,"requiredHashReadBytes":required_read_bytes,
                    "elapsedMs":elapsed.as_secs_f64()*1000.0,"logicalFilesPerSecond":operation_count as f64/elapsed.as_secs_f64().max(f64::EPSILON),
                    "actualHashReadBytesPerSecond":actual_read_bytes as f64/elapsed.as_secs_f64().max(f64::EPSILON),
                    "finalBytesAndBackupsVerified":true,"setupAndFinalVerificationTimed":false,
                    "cacheState":"Uncontrolled warm OS cache; no cache flush; recreated fixture per mode; second round reverses mode order",
                    "fixtureParentFromEnvironment":std::env::var_os("UM_ENGINE_APPLY_FIXTURE_ROOT").is_some(),"userEngineRead":false
                }));
            }
        }
    }
}

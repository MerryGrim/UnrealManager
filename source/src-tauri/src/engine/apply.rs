//! Per-operation verification and serial replacement. No file from a later
//! operation is opened early. Windows read guards deny writes and deletion until
//! each verified file's own rename; recovery keeps the existing journal format.
use super::*;
use std::{io::Read,sync::{Arc,atomic::AtomicU64,mpsc::{self,Receiver,SyncSender}}};

#[derive(Clone)]
pub(super) struct Options { pub workers:usize,pub buffer_bytes:usize,pub interval:Duration,
    #[cfg(test)]pub read_hook:Option<Arc<dyn Fn(&Path,usize)+Send+Sync>>,
}
impl Default for Options{fn default()->Self{Self{workers:1,buffer_bytes:1048576,interval:Duration::from_millis(250),#[cfg(test)]read_hook:None}}}
#[derive(Clone,Debug)]
pub(super) struct Snapshot {
    pub phase:String,pub done:u64,pub total:u64,pub current_file:Option<String>,
    pub current_file_stage:Option<&'static str>,pub current_file_done:Option<u64>,
    pub current_file_total:Option<u64>,pub read_bytes:u64,pub read_bytes_per_second:u64,
}
pub(super) struct Reporter<F:FnMut(Snapshot)> {
    notify:F,interval:Duration,last:Instant,last_bytes:u64,read:u64,last_paused:bool,
}
impl<F:FnMut(Snapshot)> Reporter<F> {
    pub fn new(notify:F,interval:Duration)->Self{Self{notify,interval,last:Instant::now(),last_bytes:0,read:0,last_paused:false}}
    pub fn basic(&mut self,phase:&str,done:u64,total:u64){
        (self.notify)(Snapshot{phase:phase.into(),done,total,current_file:None,current_file_stage:None,
            current_file_done:None,current_file_total:None,read_bytes:self.read,read_bytes_per_second:0});
    }
    fn active(&mut self,c:&Context,stats:&Stats,paused:bool,force:bool){
        if !force && self.last_paused==paused && self.last.elapsed()<self.interval{return;}
        self.last_paused=paused;
        let bytes=stats.all.load(Ordering::SeqCst);let seconds=self.last.elapsed().as_secs_f64();
        let speed=if paused||seconds<=0.0{0}else{((bytes.saturating_sub(self.last_bytes))as f64/seconds)as u64};
        self.read=bytes;self.last_bytes=bytes;self.last=Instant::now();
        let known=stats.opened.load(Ordering::SeqCst)==c.checks as u64;
        let kind=if c.kind=="readBoth"{match stats.active.load(Ordering::SeqCst){1=>"readOld",2=>"readNew",_=>"readBoth"}}else{c.kind};
        (self.notify)(Snapshot{phase:"Устанавливаем движок".into(),done:c.done,total:c.total,
            current_file:Some(c.name.clone()),current_file_stage:Some(kind),
            current_file_done:if c.kind=="replace"{None}else{Some(stats.current.load(Ordering::SeqCst))},
            current_file_total:if c.kind=="replace"||!known{None}else{Some(stats.total.load(Ordering::SeqCst))},
            read_bytes:bytes,read_bytes_per_second:speed});
    }
}
#[derive(Default)]struct Stats{all:Arc<AtomicU64>,current:AtomicU64,total:AtomicU64,opened:AtomicU64,active:std::sync::atomic::AtomicUsize,
    #[cfg(test)]hook:Option<Arc<dyn Fn(&Path,usize)+Send+Sync>>,
}
#[derive(Clone)]struct Context{name:String,root:Option<PathBuf>,done:u64,total:u64,kind:&'static str,checks:usize}
#[derive(Clone,Copy)]struct Control<'a>{paused:Option<&'a AtomicBool>,suspend:Option<&'a AtomicBool>}
impl Control<'_>{
    fn checkpoint(&self,stop:&AtomicBool)->Result<(),String>{
        loop{if stop.load(Ordering::SeqCst){return Err("Проверка установки прервана".into());}
            super::super::resume::check_suspend(self.suspend)?;
            if !self.paused.is_some_and(|v|v.load(Ordering::SeqCst)){return Ok(());}
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fn paused(&self)->bool{self.paused.is_some_and(|v|v.load(Ordering::SeqCst))}
}
#[derive(PartialEq,Eq,Debug)]struct Identity{volume:u64,id:[u8;16]}
#[cfg(windows)]fn identity(file:&fs::File)->Result<Identity,String>{
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandleEx,FileIdInfo,FILE_ID_INFO};
    let mut value:FILE_ID_INFO=unsafe{std::mem::zeroed()};
    if unsafe{GetFileInformationByHandleEx(file.as_raw_handle().cast(),FileIdInfo,
        (&mut value as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>() as u32)}==0{
        return Err("Не удалось подтвердить идентичность файла перед заменой".into());
    }
    if value.FileId.Identifier==[0;16]{return Err("Не удалось подтвердить идентичность файла перед заменой".into());}
    Ok(Identity{volume:value.VolumeSerialNumber,id:value.FileId.Identifier})
}
#[cfg(unix)]fn identity(file:&fs::File)->Result<Identity,String>{
    use std::os::unix::fs::MetadataExt;let m=file.metadata().map_err(io)?;let mut id=[0;16];id[..8].copy_from_slice(&m.ino().to_le_bytes());Ok(Identity{volume:m.dev(),id})
}
#[cfg(not(any(windows,unix)))]fn identity(_: &fs::File)->Result<Identity,String>{Err("Поддерживается Windows".into())}
fn open_read(path:&Path)->Result<fs::File,String>{
    no_links(path)?;let mut options=fs::OpenOptions::new();options.read(true);
    #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(1).custom_flags(0x08000000);}
    options.open(path).map_err(io)
}
#[cfg(windows)]fn open_identity(path:&Path)->Result<fs::File,String>{
    use std::os::windows::fs::OpenOptionsExt;
    // The original data-read guard is still held. Query the actual leaf object
    // without following a newly inserted leaf reparse point, or asking Windows
    // to open/prepare another sequential data stream just to compare its ID.
    fs::OpenOptions::new().access_mode(0x80).share_mode(1).custom_flags(0x00200000).open(path).map_err(io)
}
#[cfg(not(windows))]fn open_identity(path:&Path)->Result<fs::File,String>{open_read(path)}
struct Guard{path:PathBuf,file:fs::File,identity:Identity,size:u64}
impl Guard{
    fn open(path:&Path)->Result<Self,String>{
        let file=open_read(path)?;let metadata=file.metadata().map_err(io)?;
        if !metadata.is_file(){return Err(format!("Файл изменился после проверки: {}",path.display()));}
        let identity=identity(&file)?;Ok(Self{path:path.into(),file,identity,size:metadata.len()})
    }
    fn fresh(&self)->Result<(),String>{
        // The original handle remains held while the path is opened again.
        // FileIdInfo works on NTFS and ReFS; an unsupported identity query fails
        // closed rather than trusting size/mtime or a stale path cache.
        let current=open_identity(&self.path)?;
        if identity(&current)?!=self.identity{return Err(format!("Файл изменился после проверки: {}",self.path.display()));}Ok(())
    }
}
#[derive(Clone,Copy,PartialEq,Eq)]enum Role{OldTarget,OldBackup,NewSource,NewTarget}
struct Check{path:PathBuf,hash:String,role:Role,error:String}
struct Layout{applied:bool,backup_present:bool,checks:Vec<Check>}
struct Verified{layout:Layout,guards:Vec<(Role,Guard)>}
fn inspect(root:&Path,stage:&Path,op:&Operation,restoring:bool)->Result<Layout,String>{
    let rel=relative(&op.name)?;let target=root.join(&rel);let source=stage.join("files").join(&rel);let backup=stage.join("originals").join(&rel);
    // A fresh replacement opens and validates its hash inputs directly below;
    // avoid an existence preflight plus a duplicate ancestor scan. Recovery also
    // inspects paths that may not be hash inputs, so checks all three paths.
    if restoring{no_links(&target)?;no_links(&source)?;no_links(&backup)?;}
    let mut layout=Layout{applied:false,backup_present:false,checks:Vec::new()};
    if restoring {
        if let Some(old)=&op.old_hash {
            if backup.exists(){
                layout.backup_present=true;
                layout.checks.push(Check{path:backup,hash:old.clone(),role:Role::OldBackup,error:"Резервный файл изменён".into()});
                match &op.new_hash {
                    None=>{if target.exists(){return Err("При продолжении обнаружены внешние изменения".into());}layout.applied=true;return Ok(layout);}
                    Some(new) if target.exists()=>{layout.checks.push(Check{path:target,hash:new.clone(),role:Role::NewTarget,error:"При продолжении обнаружены внешние изменения".into()});layout.applied=true;return Ok(layout);}
                    _=>(),
                }
            }
        }else if target.exists(){
            if let Some(new)=&op.new_hash {if !source.exists(){layout.checks.push(Check{path:target,hash:new.clone(),role:Role::NewTarget,error:"При продолжении обнаружены внешние изменения".into()});layout.applied=true;return Ok(layout);}}
            return Err("При продолжении обнаружены внешние изменения".into());
        }
    }
    if !layout.backup_present {
        if let Some(old)=&op.old_hash{layout.checks.push(Check{path:target,hash:old.clone(),role:Role::OldTarget,error:format!("Файл изменился после проверки: {}",op.name)});}
        else {no_links(&target)?;if target.exists(){return Err(format!("В папке появился файл: {}",op.name));}}
    }else if target.exists(){return Err("При продолжении обнаружены внешние изменения".into());}
    if let Some(new)=&op.new_hash{layout.checks.push(Check{path:source,hash:new.clone(),role:Role::NewSource,error:"Скачанный файл изменился до установки".into()});}
    Ok(layout)
}
fn read_checked(check:Check,mut guard:Guard,buffer:&mut[u8],stats:&Stats,control:Control<'_>,stop:&AtomicBool)->Result<(Role,Guard),String>{
    control.checkpoint(stop)?;
    let mask=if matches!(check.role,Role::OldTarget|Role::OldBackup){1}else{2};stats.active.fetch_or(mask,Ordering::SeqCst);
    struct Reading<'a>(&'a std::sync::atomic::AtomicUsize,usize);impl Drop for Reading<'_>{fn drop(&mut self){self.0.fetch_and(!self.1,Ordering::SeqCst);}}
    let _reading=Reading(&stats.active,mask);
    let mut hash=Sha1::new();loop{control.checkpoint(stop)?;let n=guard.file.read(buffer).map_err(io)?;if n==0{break;}
        hash.update(&buffer[..n]);stats.current.fetch_add(n as u64,Ordering::SeqCst);stats.all.fetch_add(n as u64,Ordering::SeqCst);
        #[cfg(test)]if let Some(hook)=&stats.hook{hook(&check.path,n);}
    }
    if !format!("{:x}",hash.finalize()).eq_ignore_ascii_case(&check.hash){return Err(check.error);}
    Ok((check.role,guard))
}
fn take_guard(guards:&mut Vec<(Role,Guard)>,role:Role)->Result<Guard,String>{
    let index=guards.iter().position(|(r,_)|*r==role).ok_or("Повреждена сохранённая задача установки")?;Ok(guards.swap_remove(index).1)
}
fn replace(root:&Path,stage:&Path,op:&Operation,mut verified:Verified,control:Control<'_>,stop:&AtomicBool,parents:&mut BTreeSet<PathBuf>)->Result<(),String>{
    control.checkpoint(stop)?;
    if verified.layout.applied{for(_,guard)in &verified.guards{guard.fresh()?;}return Ok(());}
    // Old/new files that will be renamed are checked immediately before their
    // own rename below. Only a retained recovery backup needs an upfront check.
    for(role,guard)in &verified.guards{if *role==Role::OldBackup{guard.fresh()?;}}
    let rel=relative(&op.name)?;let target=root.join(&rel);let source=stage.join("files").join(&rel);let backup=stage.join("originals").join(&rel);
    if op.old_hash.is_some()&&!verified.layout.backup_present {
        let parent=backup.parent().ok_or("Путь файла")?;
        if !parents.contains(parent){no_links(&backup)?;fs::create_dir_all(parent).map_err(io)?;parents.insert(parent.to_path_buf());}
        control.checkpoint(stop)?;let guard=take_guard(&mut verified.guards,Role::OldTarget)?;guard.fresh()?;
        // Destination checks occur after any pause, while the source guard is
        // still held, immediately before our own rename.
        no_links(&backup)?;if backup.exists(){return Err("Осталась предыдущая резервная копия. Нужен откат.".into());}
        drop(guard);
        fs::rename(&target,&backup).map_err(io)?;
    }else{no_links(&target)?;if target.exists(){return Err("При продолжении обнаружены внешние изменения".into());}}
    if op.new_hash.is_some(){
        let parent=target.parent().ok_or("Путь файла")?;
        if !parents.contains(parent){no_links(&target)?;fs::create_dir_all(parent).map_err(io)?;parents.insert(parent.to_path_buf());}
        control.checkpoint(stop)?;let guard=take_guard(&mut verified.guards,Role::NewSource)?;guard.fresh()?;
        // Recheck absence immediately before the owned source is renamed.
        no_links(&target)?;if target.exists(){return Err(format!("В папке появился файл: {}",op.name));}
        drop(guard);
        fs::rename(&source,&target).map_err(io)?;
    }
    Ok(())
}
// Only large pairs use the second reader: tiny files remain direct serial work
// in the driver, avoiding scheduler round trips for each file.
const PARALLEL_PAIR_MIN_BYTES:u64=1024*1024;
struct HashWork{check:Check,guard:Guard,stats:Arc<Stats>}
struct Helper{send:SyncSender<HashWork>,receive:Receiver<Result<(Role,Guard),String>>}
#[derive(Clone)]struct View{context:Context,stats:Arc<Stats>}
struct Shared{view:Mutex<View>,generation:AtomicU64,acknowledged:AtomicU64}
impl Shared{
    fn view(&self)->View{self.view.lock().unwrap_or_else(|e|e.into_inner()).clone()}
}
enum Signal<T>{Frame(u64),Complete(Result<T,String>)}
pub(super) struct Pool<'a,T:Send>{
    helper:Option<Helper>,control:Control<'a>,stop:Arc<AtomicBool>,all:Arc<AtomicU64>,
    shared:Arc<Shared>,signals:SyncSender<Signal<T>>,context:Context,stats:Arc<Stats>,
    buffer:Vec<u8>,parents:BTreeSet<PathBuf>,every_stage:bool,
    #[cfg(test)]hook:Option<Arc<dyn Fn(&Path,usize)+Send+Sync>>,
}
impl<T:Send> Drop for Pool<'_,T>{fn drop(&mut self){self.stop.store(true,Ordering::SeqCst);self.helper.take();}}
impl<T:Send> Pool<'_,T>{
    fn publish(&self,required:bool)->Result<(),String>{
        *self.shared.view.lock().unwrap_or_else(|e|e.into_inner())=View{context:self.context.clone(),stats:self.stats.clone()};
        if required||self.every_stage {
            let generation=self.shared.generation.fetch_add(1,Ordering::SeqCst)+1;
            self.signals.send(Signal::Frame(generation)).map_err(|_|"Проверка установки прервана".to_string())?;
            while self.shared.acknowledged.load(Ordering::SeqCst)<generation {
                if self.stop.load(Ordering::SeqCst){return Err("Проверка установки прервана".into());}
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
    fn open(&self,check:&Check)->Result<Guard,String>{
        self.control.checkpoint(&self.stop)?;let guard=Guard::open(&check.path)?;
        self.stats.total.fetch_add(guard.size,Ordering::SeqCst);self.stats.opened.fetch_add(1,Ordering::SeqCst);Ok(guard)
    }
    pub fn operation(&mut self,root:&Path,stage:&Path,op:&Operation,restoring:bool,done:u64,total:u64,before_mutation:impl FnOnce())->Result<(),String>{
        self.stats=Arc::new(Stats{all:self.all.clone(),#[cfg(test)]hook:self.hook.clone(),..Default::default()});
        self.context=Context{name:op.name.clone(),root:Some(root.into()),done,total,kind:if restoring{"resumeCheck"}else{"readOld"},checks:usize::MAX};
        self.publish(done==0)?;self.control.checkpoint(&self.stop)?;
        let mut layout=inspect(root,stage,op,restoring)?;let checks=std::mem::take(&mut layout.checks);
        self.context.checks=checks.len();
        self.context.kind=if restoring{"resumeCheck"}else if checks.len()>1{"readBoth"}else if checks.first().is_some_and(|c|c.role==Role::NewSource){"readNew"}else{"readOld"};
        // Open all inputs first and retain each guard through its own rename.
        // An error opening the second input is deferred until the first SHA1
        // has been checked, preserving the previous deterministic error order.
        let inputs:Vec<_>=checks.into_iter().map(|check|{let guard=self.open(&check);(check,guard)}).collect();
        self.publish(false)?;
        let paired=self.helper.is_some()&&inputs.len()==2&&inputs.iter().all(|(_,g)|g.as_ref().is_ok_and(|g|g.size>=PARALLEL_PAIR_MIN_BYTES));
        let mut guards=Vec::new();
        if paired {
            let mut inputs=inputs.into_iter();let(first,old)=inputs.next().unwrap();let(second,new)=inputs.next().unwrap();
            self.helper.as_ref().unwrap().send.send(HashWork{check:second,guard:new?,stats:self.stats.clone()}).map_err(|_|"Проверка установки прервана".to_string())?;
            let old=read_checked(first,old?,&mut self.buffer,&self.stats,self.control,&self.stop);
            // Old errors take precedence. On error Pool Drop disconnects the
            // helper and stops its read before the enclosing scope joins.
            guards.push(old?);
            guards.push(self.helper.as_ref().unwrap().receive.recv().map_err(|_|"Проверка установки прервана".to_string())??);
        }else{
            for(check,guard)in inputs{guards.push(read_checked(check,guard?,&mut self.buffer,&self.stats,self.control,&self.stop)?);}
        }
        self.publish(false)?;
        if !layout.applied{before_mutation();}
        self.context.kind="replace";self.publish(false)?;
        replace(root,stage,op,Verified{layout,guards},self.control,&self.stop,&mut self.parents)?;
        self.context.done=done+1;self.publish(done+1==total)?;Ok(())
    }
}
pub(super) fn with_pool<T:Send,F:FnMut(Snapshot)>(options:Options,paused:Option<&AtomicBool>,suspend:Option<&AtomicBool>,reporter:&mut Reporter<F>,body:impl FnOnce(&mut Pool<'_,T>)->Result<T,String>+Send)->Result<T,String>{
    let control=Control{paused,suspend};
    #[cfg(windows)]let workers=options.workers.clamp(1,2);
    #[cfg(not(windows))]let workers=1;
    let buffer_bytes=options.buffer_bytes.clamp(65536,1048576);
    let all=Arc::new(AtomicU64::new(reporter.read));
    let result=std::thread::scope(|scope|{
        let stop=Arc::new(AtomicBool::new(false));
        struct Stop(Arc<AtomicBool>);impl Drop for Stop{fn drop(&mut self){self.0.store(true,Ordering::SeqCst);}}
        // Created before any callback; on unwind it stops the driver/helper
        // before thread::scope joins them, including paused read loops.
        let _stop=Stop(stop.clone());
        let initial_stats=Arc::new(Stats{all:all.clone(),..Default::default()});
        let initial_context=Context{name:String::new(),root:None,done:0,total:0,kind:"readOld",checks:0};
        let shared=Arc::new(Shared{view:Mutex::new(View{context:initial_context.clone(),stats:initial_stats.clone()}),generation:AtomicU64::new(0),acknowledged:AtomicU64::new(0)});
        let helper=if workers==2 {
            let(tx,rx)=mpsc::sync_channel::<HashWork>(1);let(result_tx,result_rx)=mpsc::sync_channel(1);let stop=stop.clone();
            scope.spawn(move||{let mut buffer=vec![0u8;buffer_bytes];while let Ok(work)=rx.recv(){
                let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||read_checked(work.check,work.guard,&mut buffer,&work.stats,control,&stop)));
                let panicked=result.is_err();if panicked{stop.store(true,Ordering::SeqCst);}
                let result=result.unwrap_or_else(|_|Err("Проверка установки прервана".into()));
                if result_tx.send(result).is_err()||panicked{break;}
            }});
            Some(Helper{send:tx,receive:result_rx})
        }else{None};
        let(signals,receive)=mpsc::sync_channel(1);let driver_signals=signals.clone();let driver_stop=stop.clone();let driver_shared=shared.clone();let driver_all=all.clone();
        scope.spawn(move||{
            let mut pool=Pool{helper,control,stop:driver_stop,all:driver_all,shared:driver_shared,signals:driver_signals,
                context:initial_context,stats:initial_stats,buffer:vec![0u8;buffer_bytes],parents:BTreeSet::new(),every_stage:options.interval.is_zero(),#[cfg(test)]hook:options.read_hook.clone()};
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||body(&mut pool)))
                .unwrap_or_else(|_|Err("Проверка установки прервана".into()));
            drop(pool);let _=signals.send(Signal::Complete(result));
        });
        let mut last_idle=Instant::now();let mut idle_error=None;
        loop{
            let signal=receive.recv_timeout(Duration::from_millis(100));let view=shared.view();
            if !view.context.name.is_empty(){
                let force=matches!(&signal,Ok(Signal::Frame(_)));
                reporter.active(&view.context,&view.stats,control.paused(),force);
                // The process guard stays on the coordinator so existing
                // thread-local test mocks and callback changes remain visible.
                if idle_checkpoint_due(last_idle){
                    if let Some(root)=&view.context.root{if let Err(error)=idle(root){idle_error=Some(error);stop.store(true,Ordering::SeqCst);}}
                    last_idle=Instant::now();
                }
            }
            match signal{
                Ok(Signal::Frame(generation))=>{shared.acknowledged.store(generation,Ordering::SeqCst);}
                Ok(Signal::Complete(result))=>return if let Some(error)=idle_error{Err(error)}else{result},
                Err(mpsc::RecvTimeoutError::Timeout)=>(),
                Err(_)=>return Err("Проверка установки прервана".into()),
            }
        }
    });
    reporter.read=all.load(Ordering::SeqCst);result
}

#[cfg(test)]
#[path="apply_tests.rs"]mod tests;

//! Bounded installed-file inspection. No persistent hash/link cache or writes.
use super::{io, no_links, relative, FileManifestList, Operation, Plan};
use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path, sync::{atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}, mpsc, Arc, Mutex}, time::{Duration, Instant}};

pub(super) struct Snapshot {
    pub done:u64, pub total:u64, pub processed_bytes:u64, pub current_file:Option<String>,
    pub files_per_second:f64, pub read_bytes_per_second:u64,
}
#[derive(Clone,Copy)]
enum Job<'a>{Wanted(&'a str,&'a FileManifestList,Option<&'a FileManifestList>),Removed(&'a str,&'a FileManifestList)}
impl<'a> Job<'a>{fn name(self)->&'a str{match self{Self::Wanted(n,..)|Self::Removed(n,..)=>n}}}
enum Failure{Error(String),Aborted}
impl From<String> for Failure{fn from(e:String)->Self{Self::Error(e)}}
fn stopped(cancel:&AtomicBool,abort:&impl Fn()->bool)->Result<(),Failure>{
    if cancel.load(Ordering::SeqCst){return Err(Failure::Error("Проверка остановлена".into()));}
    if abort(){return Err(Failure::Aborted);}Ok(())
}
fn jobs(plan:&Plan)->Vec<Job<'_>>{
    // A linear merge avoids a tree lookup for each unchanged manifest entry.
    let mut jobs=Vec::with_capacity(plan.files.len());let mut old=plan.old.iter().peekable();
    for(name,file)in &plan.files{
        while old.peek().is_some_and(|(n,_)|n.as_str()<name.as_str()){old.next();}
        let previous=match old.peek(){Some((n,f))if *n==name=>Some(*f),_=>None};
        jobs.push(Job::Wanted(name,file,previous));
    }
    let mut fresh=plan.files.keys().peekable();
    for(name,file)in &plan.old{
        while fresh.peek().is_some_and(|n|n.as_str()<name.as_str()){fresh.next();}
        if fresh.peek().is_some_and(|n|*n==name)||(!plan.remove_unselected&&plan.tags.contains_key(name)){continue;}
        jobs.push(Job::Removed(name,file));
    }
    jobs
}
fn inspect(job:Job<'_>,root:&Path,cancel:&AtomicBool,abort:&impl Fn()->bool,buffer:&mut[u8],mut read:impl FnMut(u64))->Result<Option<Operation>,Failure>{
    stopped(cancel,abort)?;
    let name=job.name();let path=root.join(relative(name)?);
    if let Job::Wanted(_,new,Some(old))=job{
        // Existing delta-update policy: unchanged manifest entries need a fresh
        // file-presence check, not another full content hash.
        if old.file_hash.eq_ignore_ascii_case(&new.file_hash)&&path.is_file(){return Ok(None);}
    }
    no_links(&path)?;
    // Direct open replaces exists()+open(); hash this already-checked path
    // without the second ancestor traversal performed by shared hash_file().
    let mut file=match File::open(&path){Ok(f)=>Some(f),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(io(e).into())};
    let hash=if let Some(file)=file.as_mut(){
        let mut hash=Sha1::new();loop{stopped(cancel,abort)?;let n=file.read(buffer).map_err(io)?;if n==0{break;}hash.update(&buffer[..n]);read(n as u64);}
        Some(format!("{:x}",hash.finalize()))
    }else{None};
    stopped(cancel,abort)?;
    match job{
        Job::Wanted(_,new,old)=>{
            if hash.as_ref().is_some_and(|h|h.eq_ignore_ascii_case(&new.file_hash)){return Ok(None);}
            if hash.as_ref().is_some_and(|h|old.is_none_or(|o|!o.file_hash.eq_ignore_ascii_case(h))){return Err(Failure::Error(format!("Локальный файл изменён: {name}. Обновление остановлено, файл сохранён.")));}
            Ok(Some(Operation{name:name.into(),old_hash:hash,new_hash:Some(new.file_hash.to_ascii_lowercase())}))
        },
        Job::Removed(_,old)=>match hash{None=>Ok(None),Some(hash)=>{if !hash.eq_ignore_ascii_case(&old.file_hash){return Err(Failure::Error(format!("Удалённый автором файл изменён локально: {name}")));}Ok(Some(Operation{name:name.into(),old_hash:Some(hash),new_hash:None}))}},
    }
}
struct Reporter<'a,F:FnMut(Snapshot)>{done:u64,total:u64,bytes:u64,current:Option<&'a str>,last:Instant,last_done:u64,last_bytes:u64,interval:Duration,send:F}
impl<'a,F:FnMut(Snapshot)> Reporter<'a,F>{
    fn emit(&mut self,force:bool){let elapsed=self.last.elapsed();if !force&&elapsed<self.interval{return;}
        let secs=elapsed.as_secs_f64();let rate=|n:u64|if secs>0.0{n as f64/secs}else{0.0};
        (self.send)(Snapshot{done:self.done,total:self.total,processed_bytes:self.bytes,current_file:self.current.map(str::to_owned),files_per_second:rate(self.done-self.last_done),read_bytes_per_second:rate(self.bytes-self.last_bytes).min(u64::MAX as f64)as u64});
        self.last=Instant::now();self.last_done=self.done;self.last_bytes=self.bytes;
    }
}
pub(super) fn operations(plan:&Plan,cancel:&AtomicBool,workers:usize,progress:impl FnMut(Snapshot))->Result<Vec<Operation>,String>{
    operations_with_interval(plan,cancel,workers,Duration::from_millis(250),progress)
}
fn operations_with_interval(plan:&Plan,cancel:&AtomicBool,workers:usize,interval:Duration,progress:impl FnMut(Snapshot))->Result<Vec<Operation>,String>{
    let root=super::target(&plan.info.path)?;let jobs=jobs(plan);
    let mut reporter=Reporter{done:0,total:jobs.len()as u64,bytes:0,current:jobs.first().map(|j|j.name()),last:Instant::now(),last_done:0,last_bytes:0,interval,send:progress};reporter.emit(true);
    let workers=match workers{2=>2,4=>4,_=>1}.min(jobs.len().max(1));
    let result=if workers==1{
        let mut buffer=vec![0u8;65536];let mut output=Vec::new();
        (||{for &job in &jobs{reporter.current=Some(job.name());let result=inspect(job,&root,cancel,&||false,&mut buffer,|n|{reporter.bytes+=n;reporter.emit(false);});match result{Ok(Some(op))=>output.push(op),Ok(None)=>(),Err(Failure::Error(e))=>return Err(e),Err(Failure::Aborted)=>return Err("Проверка остановлена".into())};reporter.done+=1;reporter.emit(false);}Ok(output)})()
    }else{
        parallel(&jobs,&root,cancel,workers,&mut reporter)
    };
    if result.is_ok(){reporter.current=None;}reporter.emit(true);result
}
fn parallel<'a,F:FnMut(Snapshot)>(jobs:&[Job<'a>],root:&Path,cancel:&AtomicBool,workers:usize,reporter:&mut Reporter<'a,F>)->Result<Vec<Operation>,String>{
    std::thread::scope(|scope|{
        let (job_send,job_receive)=mpsc::channel::<usize>();let job_receive=Arc::new(Mutex::new(job_receive));
        let (result_send,result_receive)=mpsc::channel();
        let stop=Arc::new(AtomicBool::new(false));let first_error=Arc::new(AtomicUsize::new(usize::MAX));let bytes=Arc::new(AtomicU64::new(0));
        let current=Arc::new((0..workers).map(|_|Mutex::new(None::<usize>)).collect::<Vec<_>>());
        let mut handles=Vec::new();
        for worker in 0..workers{
            let receiver=job_receive.clone();let send=result_send.clone();let stop=stop.clone();let first_error=first_error.clone();let bytes=bytes.clone();let current=current.clone();
            handles.push(scope.spawn(move||{let mut buffer=vec![0u8;65536];loop{
                let index=match receiver.lock(){Ok(r)=>match r.recv(){Ok(i)=>i,Err(_)=>break},Err(_)=>break};
                if stop.load(Ordering::SeqCst)||index>first_error.load(Ordering::SeqCst){break;}
                if let Ok(mut slot)=current[worker].lock(){*slot=Some(index);}
                let result=inspect(jobs[index],root,cancel,&||stop.load(Ordering::SeqCst)||index>first_error.load(Ordering::SeqCst),&mut buffer,|n|{bytes.fetch_add(n,Ordering::SeqCst);});
                if let Ok(mut slot)=current[worker].lock(){*slot=None;}
                match result{Err(Failure::Aborted)=>break,Err(Failure::Error(e))=>{first_error.fetch_min(index,Ordering::SeqCst);if send.send((index,Err(e))).is_err(){break;}},Ok(op)=>if send.send((index,Ok(op))).is_err(){break;}}
            }}));
        }
        drop(result_send);
        // At most 64 jobs/results may be ahead of the ordered frontier, even
        // when an early large file takes longer than later tiny files.
        let mut scheduled=0usize;let mut frontier=0usize;let mut pending=BTreeMap::new();let mut output=Vec::new();
        let result=(||{
            loop{
                while scheduled<jobs.len()&&scheduled<frontier.saturating_add(64)&&scheduled<=first_error.load(Ordering::SeqCst){job_send.send(scheduled).map_err(|_|"Проверка прервана")?;scheduled+=1;}
                if frontier==jobs.len(){return Ok(output);}
                if cancel.load(Ordering::SeqCst){return Err("Проверка остановлена".into());}
                match result_receive.recv_timeout(interval_for_wait(reporter.interval)){
                    Ok((index,result))=>{if result.is_ok(){reporter.done+=1;}pending.insert(index,result);while let Some(result)=pending.remove(&frontier){match result?{Some(op)=>output.push(op),None=>()};frontier+=1;}},
                    Err(mpsc::RecvTimeoutError::Timeout)=>(),Err(_)=>return Err("Проверка прервана".into()),
                }
                reporter.bytes=bytes.load(Ordering::SeqCst);reporter.current=current.iter().filter_map(|slot|slot.lock().ok().and_then(|v|*v)).min().map(|i|jobs[i].name());reporter.emit(false);
            }
        })();
        stop.store(true,Ordering::SeqCst);drop(job_send);drop(result_receive);
        let mut panicked=false;for handle in handles{if handle.join().is_err(){panicked=true;}}
        reporter.bytes=bytes.load(Ordering::SeqCst);if panicked{Err("Проверка прервана".into())}else{result}
    })
}
fn interval_for_wait(interval:Duration)->Duration{if interval.is_zero(){Duration::from_millis(1)}else{interval.min(Duration::from_millis(100))}}

#[cfg(test)]
#[path="verification_tests.rs"]
mod tests;

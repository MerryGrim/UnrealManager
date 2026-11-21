//! Persistent download intentions only. One worker, event-driven, no polling/service.
use serde::{Deserialize,Serialize};
use std::{collections::BTreeSet,fs,io::Read,path::{Path,PathBuf},sync::{Mutex,atomic::Ordering}};
use tauri::{Emitter,Manager};
use crate::vault::{Package,Progress,VaultState};
const LIMIT:usize=128;
const MAX_BYTES:u64=1024*1024;
pub(crate) const STALE:&str="Очередь изменилась. Повторите действие.";
const BAD:&str="Повреждена очередь загрузок. Существующие задачи сохранены.";
const ACCOUNT:&str="Аккаунт изменился. Удалите задание и добавьте его заново.";
const VAULT:&str="Папка Vault изменилась. Удалите задание и добавьте его заново.";
const PARTIAL:&str="Продолжите сохранённую загрузку в списке задач.";
const NO_MANUAL_MATCH:&str="No matching interrupted queue request.";
const SAVE_ERROR:&str="Не удалось сохранить очередь. Существующие задачи сохранены.";
const CANCEL_ERROR:&str="Не удалось удалить запись сохранённой загрузки. Файлы сохранены. Повторите отмену.";
fn failed_entry(entry:&mut Entry,partial:bool,error:&str){
 entry.status=if partial{Status::Interrupted}else{Status::NeedsAction};
 entry.error=Some(if error==crate::epic::AUTH_REQUIRED{crate::epic::AUTH_REQUIRED}else if partial||error==PARTIAL{PARTIAL}else if error==VAULT{VAULT}else if error==ACCOUNT{ACCOUNT}else if error=="Недостаточно свободного места."{"Недостаточно свободного места."}else{"Не удалось выполнить загрузку. Повторите попытку."}.into());
}
static WAKE:tokio::sync::Notify=tokio::sync::Notify::const_new();
pub(crate) fn wake(){WAKE.notify_one();}

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Request{pub asset_id:String,pub artifact_id:String,pub selected_engine:String,pub title:String}
#[derive(Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
enum Status{Pending,Preparing,Downloading,NeedsAction,Interrupted}
#[derive(Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
enum Control{Defer,Cancel}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Entry{id:String,request:Request,requested_vault_path:String,account_key:String,status:Status,package_id:Option<String>,error:Option<String>,#[serde(default,skip_serializing_if="Option::is_none")]control:Option<Control>,#[serde(default)]resume_saved:bool,#[serde(default)]owned_transfer:bool,#[serde(default,skip_serializing_if="Option::is_none")]saved_progress:Option<Progress>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Disk{schema:u32,revision:u64,items:Vec<Entry>}
impl Default for Disk{fn default()->Self{Self{schema:1,revision:0,items:vec![]}}}
struct Inner{disk:Disk,paused:bool,user_paused:bool,stopping:bool,error:Option<String>,progress:Option<Progress>,last_emit:Option<std::time::Instant>}
pub struct QueueState{inner:Mutex<Inner>,path:PathBuf}
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct QueueItem{id:String,#[serde(flatten)]request:Request,requested_vault_path:String,status:Status,package_id:Option<String>,error:Option<String>,progress:Option<Progress>,control_pending:Option<Control>,can_defer:bool,can_cancel:bool}
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Snapshot{revision:u64,paused:bool,active_id:Option<String>,items:Vec<QueueItem>,error:Option<String>,completed:Option<Package>}
fn valid_request(r:&Request)->bool{
 let identifier=|s:&str|!s.is_empty()&&s.len()<=512&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b));
 identifier(&r.asset_id)&&identifier(&r.artifact_id)&&!r.selected_engine.is_empty()&&r.selected_engine.len()<=100&&r.selected_engine.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b))&&!r.title.trim().is_empty()&&r.title.len()<=1024&&!r.title.chars().any(char::is_control)
}
fn validate(d:&Disk)->Result<(),String>{
 if d.schema!=1||d.items.len()>LIMIT{return Err(BAD.into());}
 let mut ids=BTreeSet::new();let mut requests=BTreeSet::new();let mut active=0;
 for e in &d.items{
  if uuid::Uuid::parse_str(&e.id).is_err()||!ids.insert(&e.id)||!valid_request(&e.request)||e.requested_vault_path.len()>2000||!Path::new(&e.requested_vault_path).is_absolute()||e.account_key.len()!=64||!e.account_key.bytes().all(|b|b.is_ascii_hexdigit())||e.package_id.as_ref().is_some_and(|id|id.len()!=40||!id.bytes().all(|b|b.is_ascii_hexdigit()))||e.error.as_ref().is_some_and(|v|v.len()>512){return Err(BAD.into());}
  if !requests.insert((&e.request.asset_id,&e.request.artifact_id,&e.request.selected_engine,&e.account_key,&e.requested_vault_path)){return Err(BAD.into());}
  if matches!(e.status,Status::Preparing|Status::Downloading){active+=1;}
 }if active>1{return Err(BAD.into());}Ok(())
}
fn read_disk(path:&Path)->Result<Disk,String>{
 crate::vault::no_links(path)?;
 let file=match fs::File::open(path){Ok(f)=>f,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Disk::default()),Err(_)=>return Err(BAD.into())};
 if file.metadata().map_err(|_|BAD)?.len()>MAX_BYTES{return Err(BAD.into());}
 let mut bytes=vec![];file.take(MAX_BYTES+1).read_to_end(&mut bytes).map_err(|_|BAD)?;if bytes.len()as u64>MAX_BYTES{return Err(BAD.into());}
 let mut d:Disk=serde_json::from_slice(&bytes).map_err(|_|BAD)?;validate(&d)?;
 for e in &mut d.items{if matches!(e.status,Status::Preparing|Status::Downloading){match e.control{
  Some(Control::Defer)=>{e.status=Status::Pending;e.resume_saved=e.owned_transfer&&e.package_id.is_some();e.control=None;e.error=None;},
  Some(Control::Cancel)=>{e.status=Status::Pending;e.error=None;},
  None=>{e.status=Status::Interrupted;e.error=Some(PARTIAL.into());}
 }}}
 Ok(d)
}
fn snapshot(i:&Inner)->Snapshot{
 let active=i.disk.items.iter().find(|e|matches!(e.status,Status::Preparing|Status::Downloading)).map(|e|e.id.clone());
 Snapshot{revision:i.disk.revision,paused:i.paused,active_id:active.clone(),items:i.disk.items.iter().map(|e|{
  let progress=if active.as_ref()==Some(&e.id){i.progress.clone().or_else(||e.saved_progress.clone())}else{e.saved_progress.clone()};
  let control=active.as_ref()==Some(&e.id)&&e.control.is_none()&&(e.status==Status::Preparing||progress.as_ref().is_none_or(|p|p.can_pause));
  let can_defer=control&&i.disk.items.iter().any(|other|other.id!=e.id&&other.status==Status::Pending&&other.control.is_none());
  QueueItem{id:e.id.clone(),request:e.request.clone(),requested_vault_path:e.requested_vault_path.clone(),status:e.status,package_id:e.package_id.clone(),error:e.error.clone(),progress,control_pending:e.control,can_defer,can_cancel:control}
 }).collect(),error:i.error.clone(),completed:None}
}
impl QueueState{
 fn load(path:PathBuf)->Self{let result=read_disk(&path);let error=result.as_ref().err().cloned();let disk=result.unwrap_or_default();let restored=!disk.items.is_empty();Self{path,inner:Mutex::new(Inner{disk,paused:true,user_paused:restored,stopping:false,error,progress:None,last_emit:None})}}
 fn get(&self)->Result<Snapshot,String>{let i=self.inner.lock().map_err(|_|BAD)?;Ok(snapshot(&i))}
 fn transaction(&self,change:impl FnOnce(&mut Disk,&mut bool,&mut bool)->Result<(),String>)->Result<Snapshot,String>{
  self.transact(false,change)
 }
 fn transact(&self,finishing:bool,change:impl FnOnce(&mut Disk,&mut bool,&mut bool)->Result<(),String>)->Result<Snapshot,String>{
  self.transact_commit(finishing,change,|_|{})
 }
 fn transact_commit(&self,finishing:bool,change:impl FnOnce(&mut Disk,&mut bool,&mut bool)->Result<(),String>,committed:impl FnOnce(&mut Inner))->Result<Snapshot,String>{
  let mut i=self.inner.lock().map_err(|_|BAD)?;if let Some(e)=&i.error{return Err(e.clone());}if i.stopping&&!finishing{return Err("Программа уже завершает работу.".into());}
  let mut d=i.disk.clone();let(mut paused,mut user_paused)=(i.paused,i.user_paused);change(&mut d,&mut paused,&mut user_paused)?;d.revision=d.revision.checked_add(1).ok_or(BAD)?;validate(&d)?;
  let bytes=serde_json::to_vec(&d).map_err(|_|BAD)?;if bytes.len()as u64>MAX_BYTES{return Err(BAD.into());}
  crate::vault::no_links(&self.path)?;crate::engine::install::atomic(&self.path,&bytes).map_err(|_|"Не удалось сохранить очередь. Существующие задачи сохранены.")?;
  i.disk=d;i.paused=paused;i.user_paused=user_paused;committed(&mut i);if !i.disk.items.iter().any(|e|matches!(e.status,Status::Preparing|Status::Downloading)){i.progress=None;i.last_emit=None;}Ok(snapshot(&i))
 }
 fn eligible(&self)->bool{self.inner.lock().map(|i|!i.stopping&&i.error.is_none()&&!i.disk.items.iter().any(active)&&
  (i.disk.items.iter().any(|e|e.control==Some(Control::Cancel))||!i.paused&&i.disk.items.first().is_some_and(|e|e.status==Status::Pending))).unwrap_or(false)}
 fn request_control(&self,vault:&VaultState,id:&str,expected:u64,control:Control,before:Option<&str>)->Result<Snapshot,String>{
  let _gate=vault.control_gate.lock().map_err(|_|BAD)?;
  if !vault.queue_control_open.load(Ordering::SeqCst){return Err("На этом этапе остановка недоступна.".into());}
  let progress=self.inner.lock().map_err(|_|BAD)?.progress.clone();
  self.transact_commit(false,|d,_,_|{
   let index=active_index(d,id,expected)?;
   if control==Control::Defer {
    if before.is_some_and(|target|target==id||!d.items.iter().any(|e|e.id==target&&e.status==Status::Pending&&e.control.is_none())){return Err(STALE.into());}
    let mut entry=d.items.remove(index);entry.control=Some(control);entry.saved_progress=progress.clone();
    let position=before.and_then(|target|d.items.iter().position(|e|e.id==target)).unwrap_or(d.items.len());
    if !d.items[..position].iter().any(|e|e.status==Status::Pending&&e.control.is_none()){return Err("Верните загрузку после другого задания очереди.".into());}
    d.items.insert(position,entry);
   }else{d.items[index].control=Some(control);d.items[index].saved_progress=progress.clone();}
   Ok(())
  },|_|vault.cancel.store(true,Ordering::SeqCst))
 }
 fn pause_active(&self,vault:&VaultState,id:&str,expected:u64,paused:bool)->Result<Snapshot,String>{
  let _gate=vault.control_gate.lock().map_err(|_|BAD)?;
  self.transact_commit(false,|d,_,_|{active_index(d,id,expected)?;if !crate::vault::queue_pause_available(vault){return Err("На этом этапе пауза недоступна.".into());}Ok(())},|i|{
   crate::vault::queue_pause_flag(vault,paused);i.progress=i.progress.take().map(|p|p.queue_pause(paused));
  })
 }
 fn manual_completed(&self,id:&str,asset:&str,package_path:&Path,vault:&Path)->Option<Snapshot>{
  // Only verified native download success reaches this seam. A package in an
  // external/previous Vault cannot complete a request bound to the current one.
  if package_path!=vault.join(id){return None;}
  let matches=|e:&Entry|matches!(e.status,Status::Interrupted|Status::NeedsAction)&&e.package_id.as_deref()==Some(id)&&e.request.asset_id==asset&&Path::new(&e.requested_vault_path)==vault;
  if !self.inner.lock().ok()?.disk.items.iter().any(matches){return None;}
  let result=self.transact(true,|d,_,_|{let before=d.items.len();d.items.retain(|e|!matches(e));if d.items.len()==before{return Err(NO_MANUAL_MATCH.into());}Ok(())});
  match result{
   Ok(s)=>Some(s),
   Err(e)if e==NO_MANUAL_MATCH=>None,
   Err(_)=>{let mut i=self.inner.lock().ok()?;i.paused=true;if i.error.is_none(){i.error=Some(SAVE_ERROR.into());}Some(snapshot(&i))}
  }
 }
 fn progress(&self,app:&tauri::AppHandle,id:&str,p:Progress){
  let result=self.inner.lock().ok().and_then(|mut i|{if !i.disk.items.iter().any(|e|e.id==id&&e.status==Status::Downloading){return None;}let force=i.progress.as_ref().is_none_or(|old|old.phase!=p.phase||old.paused!=p.paused||old.can_pause!=p.can_pause);i.progress=Some(p);if !force&&i.last_emit.is_some_and(|t|t.elapsed()<std::time::Duration::from_millis(250)){return None;}i.last_emit=Some(std::time::Instant::now());Some(snapshot(&i))});
  if let Some(s)=result{emit(app,s);}
 }
}
fn emit(app:&tauri::AppHandle,s:Snapshot){let _=app.emit("vault-queue-changed",s);}
fn changed(app:&tauri::AppHandle,s:Snapshot)->Snapshot{emit(app,s.clone());wake();s}
pub(crate) fn manual_download_completed(app:&tauri::AppHandle,package:&Package,asset_id:&str,package_path:&str){
 let Some(queue)=app.try_state::<QueueState>()else{return;};
 if let Some(mut s)=queue.manual_completed(&package.id,asset_id,Path::new(package_path),&crate::storage::vault()){
  // Queue persistence failure is visible without turning verified files into
  // a failed download or losing the completion event/history refresh.
  s.completed=Some(package.clone());emit(app,s);wake();
 }
}
fn revision(d:&Disk,expected:u64)->Result<(),String>{if d.revision!=expected{Err(STALE.into())}else{Ok(())}}
fn active(e:&Entry)->bool{matches!(e.status,Status::Preparing|Status::Downloading)}
fn active_index(d:&Disk,id:&str,expected:u64)->Result<usize,String>{revision(d,expected)?;d.items.iter().position(|e|e.id==id&&active(e)&&e.control.is_none()).ok_or_else(||STALE.into())}
fn enqueue(d:&mut Disk,paused:&mut bool,user_paused:bool,request:Request,path:String,account:String)->Result<(),String>{
 if !valid_request(&request){return Err("Некорректное задание очереди.".into());}if d.items.len()>=LIMIT{return Err("Очередь заполнена.".into());}
 if d.items.iter().any(|e|e.request.asset_id==request.asset_id&&e.request.artifact_id==request.artifact_id&&e.request.selected_engine==request.selected_engine&&e.account_key==account&&e.requested_vault_path==path){return Err("Эта версия уже добавлена в очередь.".into());}
 d.items.push(Entry{id:uuid::Uuid::new_v4().to_string(),request,requested_vault_path:path,account_key:account,status:Status::Pending,package_id:None,error:None,control:None,resume_saved:false,owned_transfer:false,saved_progress:None});if !user_paused{*paused=false;}Ok(())
}
fn reorder(d:&mut Disk,ids:&[String],expected:u64)->Result<(),String>{
 revision(d,expected)?;let pending=d.items.iter().filter(|e|e.status==Status::Pending).map(|e|e.id.clone()).collect::<BTreeSet<_>>();
 if ids.len()!=pending.len()||ids.iter().collect::<BTreeSet<_>>().len()!=ids.len()||ids.iter().any(|id|!pending.contains(id)){return Err(STALE.into());}
 let ordered=ids.iter().map(|id|d.items.iter().find(|e|e.id==*id).unwrap().clone()).collect::<Vec<_>>();let mut next=ordered.into_iter();for e in &mut d.items{if e.status==Status::Pending{*e=next.next().unwrap();}}Ok(())
}
fn claim(d:&mut Disk,paused:bool)->Result<Entry,String>{
 if d.items.iter().any(active){return Err(STALE.into());}
 let index=d.items.iter().position(|e|e.control==Some(Control::Cancel))
  .or_else(||if !paused&&d.items.first().is_some_and(|e|e.status==Status::Pending){Some(0)}else{None}).ok_or(STALE)?;
 let e=&mut d.items[index];e.status=Status::Preparing;e.error=None;Ok(e.clone())
}
fn settle(d:&mut Disk,id:&str,outcome:Result<Package,String>,progress:Option<Progress>,partial:bool,forget:impl FnOnce(&Entry)->Result<(),String>)->Result<Option<Package>,String>{
 let index=d.items.iter().position(|e|e.id==id).ok_or(STALE)?;
 match outcome{
  // Once the final commit gate has closed, completion wins over a late control.
  Ok(package)=>{d.items.remove(index);Ok(Some(package))},
  Err(error)=>{let e=&mut d.items[index];match e.control{
   Some(Control::Cancel)=>{if forget(e).is_ok(){d.items.remove(index);}else{e.control=None;e.status=Status::NeedsAction;e.resume_saved=partial;e.error=Some(CANCEL_ERROR.into());}},
   Some(Control::Defer)=>{e.control=None;e.status=Status::Pending;e.resume_saved=e.owned_transfer&&partial;e.error=None;e.saved_progress=progress.or_else(||e.saved_progress.clone());},
   None=>failed_entry(e,partial,&error),
  }Ok(None)}
 }
}
fn forget_transfer(e:&Entry)->Result<(),String>{
 if e.owned_transfer{if let Some(id)=&e.package_id{crate::vault::transfers::forget_queued(&e.request,id,Path::new(&e.requested_vault_path))?;}}Ok(())
}
fn settle_ordered(d:&mut Disk,paused:&mut bool,user_paused:&mut bool,id:&str,outcome:Result<Package,String>,progress:Option<Progress>,partial:bool,forget:impl FnOnce(&Entry)->Result<(),String>)->Result<Option<Package>,String>{
 let deferred=d.items.iter().any(|e|e.id==id&&e.control==Some(Control::Defer));
 let completed=settle(d,id,outcome,progress,partial,forget)?;
 if deferred&&d.items.iter().position(|e|e.id==id&&e.status==Status::Pending).is_some_and(|index|!d.items[..index].iter().any(|e|e.status==Status::Pending&&e.control.is_none())){
  // Predecessors can be removed while this worker is stopping. Do not restart
  // the same transfer immediately when its requested place becomes the head.
  *paused=true;*user_paused=true;
 }
 Ok(completed)
}
#[tauri::command]
pub fn vault_queue_get(state:tauri::State<'_,QueueState>)->Result<Snapshot,String>{state.get()}
#[tauri::command]
pub async fn vault_queue_enqueue(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,epic:tauri::State<'_,crate::epic::EpicState>,request:Request)->Result<Snapshot,String>{
 let _operation=crate::app_updates::operation()?;if !valid_request(&request){return Err("Некорректное задание очереди.".into());}
 let account=crate::epic::queue_account(&app,&epic).await?;let path=crate::storage::vault().to_string_lossy().into_owned();
 let s=state.transaction(move|d,paused,user_paused|enqueue(d,paused,*user_paused,request,path,account))?;Ok(changed(&app,s))
}
#[tauri::command]
pub fn vault_queue_remove(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,id:String,expected_revision:u64)->Result<Snapshot,String>{
 let s=state.transaction(|d,_,_|{revision(d,expected_revision)?;let e=d.items.iter_mut().find(|e|e.id==id).ok_or(STALE)?;if active(e){return Err("Сначала остановите активную загрузку.".into());}
  if e.owned_transfer&&e.resume_saved&&e.package_id.is_some(){e.control=Some(Control::Cancel);e.status=Status::Pending;e.error=None;}else{d.items.retain(|e|e.id!=id);}Ok(())})?;Ok(changed(&app,s))
}
#[tauri::command]
pub fn vault_queue_pause_active(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,vault:tauri::State<'_,VaultState>,id:String,paused:bool,expected_revision:u64)->Result<Snapshot,String>{
 let _operation=crate::app_updates::operation()?;let s=state.pause_active(&vault,&id,expected_revision,paused)?;Ok(changed(&app,s))
}
#[tauri::command]
pub fn vault_queue_cancel_active(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,vault:tauri::State<'_,VaultState>,id:String,expected_revision:u64)->Result<Snapshot,String>{
 let _operation=crate::app_updates::operation()?;let s=state.request_control(&vault,&id,expected_revision,Control::Cancel,None)?;Ok(changed(&app,s))
}
#[tauri::command]
pub fn vault_queue_defer_active(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,vault:tauri::State<'_,VaultState>,id:String,before_id:Option<String>,expected_revision:u64)->Result<Snapshot,String>{
 let _operation=crate::app_updates::operation()?;let s=state.request_control(&vault,&id,expected_revision,Control::Defer,before_id.as_deref())?;Ok(changed(&app,s))
}
#[tauri::command]
pub fn vault_queue_reorder(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,ids:Vec<String>,expected_revision:u64)->Result<Snapshot,String>{let s=state.transaction(|d,_,_|reorder(d,&ids,expected_revision))?;Ok(changed(&app,s))}
#[tauri::command]
pub fn vault_queue_set_paused(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,paused:bool)->Result<Snapshot,String>{let s=state.transaction(|_,p,u|{*p=paused;*u=paused;Ok(())})?;Ok(changed(&app,s))}
#[tauri::command]
pub fn vault_queue_retry(app:tauri::AppHandle,state:tauri::State<'_,QueueState>,id:String,expected_revision:u64)->Result<Snapshot,String>{
 let s=state.transaction(|d,_,_|{revision(d,expected_revision)?;let e=d.items.iter_mut().find(|e|e.id==id).ok_or(STALE)?;if !matches!(e.status,Status::NeedsAction|Status::Interrupted){return Err(STALE.into());}if e.package_id.as_ref().is_some_and(|id|crate::vault::transfers::has_saved_task(id).unwrap_or(true)){return Err(PARTIAL.into());}e.status=Status::Pending;e.error=None;Ok(())})?;Ok(changed(&app,s))
}
pub(crate) fn stop_dispatch(app:&tauri::AppHandle){if let Some(q)=app.try_state::<QueueState>(){if let Ok(mut i)=q.inner.lock(){i.stopping=true;i.paused=true;emit(app,snapshot(&i));}}wake();}
pub(crate) fn resume_dispatch(app:&tauri::AppHandle){if let Some(q)=app.try_state::<QueueState>(){if let Ok(mut i)=q.inner.lock(){i.stopping=false;}}wake();}
async fn run_one(app:&tauri::AppHandle)->bool{
 let queue=app.state::<QueueState>();let vault=app.state::<VaultState>();let engine=app.state::<crate::engine::EngineState>();
 if !queue.eligible()||crate::app_updates::applying()||engine.active.load(Ordering::SeqCst)||crate::vault::queue_plan_reserved(&vault){return false;}
 let Ok(_operation)=crate::app_updates::operation()else{return false;};
 let Some(_lease)=crate::vault::queue_lease(&vault,&engine)else{return false;};
 // Reset before publishing the claimed ID: a control accepted immediately after
 // claim must never be erased by initialization of this transfer.
 if crate::vault::queue_begin_control(&vault).is_err(){return false;}
 let mut claimed=None;
 let s=queue.transaction(|d,paused,_|{claimed=Some(claim(d,*paused)?);Ok(())});
 let s=match s{Ok(s)=>s,Err(error)=>{crate::vault::queue_end_control(&vault);if error!=STALE{if let Ok(mut i)=queue.inner.lock(){if !i.stopping{i.paused=true;i.error=Some("Не удалось сохранить очередь. Существующие задачи сохранены.".into());emit(app,snapshot(&i));}}}return false;}};emit(app,s);let entry=claimed.unwrap();
 let outcome=async{
  if entry.control==Some(Control::Cancel){return Err("Загрузка отменена.".into());}
  if Path::new(&entry.requested_vault_path)!=crate::storage::vault(){return Err(VAULT.to_string());}
  let epic=app.state::<crate::epic::EpicState>();if crate::vault::queue_wait(&vault,crate::epic::queue_account(app,&epic)).await??!=entry.account_key{return Err(ACCOUNT.into());}
  let sink_app=app.clone();let sink_id=entry.id.clone();
  crate::vault::queue_download(app,&epic,&vault,&entry.request,&entry.account_key,if entry.resume_saved&&entry.owned_transfer{entry.package_id.as_deref()}else{None},|package,owned|{
   let s=queue.transaction(|d,_,_|{let e=d.items.iter_mut().find(|e|e.id==entry.id).ok_or(STALE)?;if e.control.is_some(){return Err("Загрузка остановлена.".into());}e.package_id=Some(package.id.clone());e.owned_transfer=owned;e.status=Status::Downloading;Ok(())})?;emit(app,s);Ok(())
  },std::sync::Arc::new(move|p|sink_app.state::<QueueState>().progress(&sink_app,&sink_id,p))).await
 }.await;
 crate::vault::queue_end_control(&vault);
 crate::vault::discard_plan(&vault);
 let last_progress=queue.inner.lock().ok().and_then(|i|i.progress.clone());
 let mut completed=None;
 let result=queue.transact(true,|d,paused,user_paused|{
  let e=d.items.iter().find(|e|e.id==entry.id).ok_or(STALE)?;
  let partial=e.package_id.as_ref().is_some_and(|id|crate::vault::transfers::has_saved_task(id).unwrap_or(true));
  completed=settle_ordered(d,paused,user_paused,&entry.id,outcome,last_progress,partial,forget_transfer)?;Ok(())});
 match result{Ok(mut s)=>{s.completed=completed;emit(app,s);true},Err(_)=>{if let Ok(mut i)=queue.inner.lock(){i.paused=true;i.error=Some("Не удалось сохранить очередь. Существующие задачи сохранены.".into());emit(app,snapshot(&i));}false}}
}
pub(crate) fn setup(app:&tauri::AppHandle){
 app.manage(QueueState::load(crate::storage::settings().join("vault-queue.json")));
 let handle=app.clone();tauri::async_runtime::spawn(async move{loop{WAKE.notified().await;while run_one(&handle).await{}}});
 wake();
}

#[cfg(test)]mod tests{
 use super::*;
 fn root()->PathBuf{std::env::temp_dir().join(format!("um-queue-{}",uuid::Uuid::new_v4()))}
 fn request(id:&str)->Request{Request{asset_id:format!("asset-{id}"),artifact_id:format!("artifact-{id}"),selected_engine:"5.7".into(),title:format!("Synthetic {id}")}}
 fn add(q:&QueueState,id:&str)->Snapshot{q.transaction(|d,p,u|enqueue(d,p,*u,request(id),"X:/Fixture/Vault".into(),"a".repeat(64))).unwrap()}
 fn active_queue(path:PathBuf)->(QueueState,VaultState,Snapshot){
  let q=QueueState::load(path);for id in ["one","two","three"]{add(&q,id);}
  let s=q.transaction(|d,_,_|{d.items[0].status=Status::Downloading;d.items[0].package_id=Some("b".repeat(40));d.items[0].owned_transfer=true;Ok(())}).unwrap();
  q.inner.lock().unwrap().progress=Some(serde_json::from_value(serde_json::json!({"phase":"Загружаем и проверяем файлы","done":40,"total":100,"filesDone":1,"filesTotal":3,"networkBytes":60,"bytesPerSecond":20,"paused":false,"canPause":true})).unwrap());
  (q,crate::vault::queue_test_state(),s)
 }
 #[test]fn active_defer_preserves_progress_orders_following_dispatch_and_rejects_stale_controls(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=s.items[0].id.clone();let second=s.items[1].id.clone();let third=s.items[2].id.clone();
  let before=fs::read(&q.path).unwrap();
  for (id,rev,target) in [(id.as_str(),s.revision-1,None),(second.as_str(),s.revision,None),(id.as_str(),s.revision,Some(second.as_str()))]{assert!(q.request_control(&vault,id,rev,Control::Defer,target).is_err());assert_eq!(fs::read(&q.path).unwrap(),before);assert!(!vault.cancel.load(Ordering::SeqCst));}
  let waiting=q.request_control(&vault,&id,s.revision,Control::Defer,Some(&third)).unwrap();
  assert_eq!(waiting.items.iter().map(|e|e.id.as_str()).collect::<Vec<_>>(),vec![second.as_str(),id.as_str(),third.as_str()]);assert!(vault.cancel.load(Ordering::SeqCst));assert!(waiting.items[1].control_pending==Some(Control::Defer));assert!(!q.eligible());
  assert_eq!(serde_json::to_value(waiting.items[1].progress.as_ref().unwrap()).unwrap()["done"],40);
  assert!(q.request_control(&vault,&id,s.revision,Control::Cancel,None).is_err());
  let progress=q.inner.lock().unwrap().progress.clone();
  let pending=q.transact(true,|d,_,_|{assert!(settle(d,&id,Err("stopped".into()),progress,true,|_|panic!("Defer must not forget metadata")).unwrap().is_none());Ok(())}).unwrap();
  assert!(pending.active_id.is_none());assert!(pending.items[1].status==Status::Pending);assert!(q.inner.lock().unwrap().disk.items[1].resume_saved);assert!(q.eligible());
  let restored=QueueState::load(q.path.clone());let restored_snapshot=restored.get().unwrap();assert!(restored_snapshot.paused);assert_eq!(restored_snapshot.items.iter().map(|e|e.id.as_str()).collect::<Vec<_>>(),vec![second.as_str(),id.as_str(),third.as_str()]);
  restored.transaction(|d,p,u|{*p=false;*u=false;assert_eq!(claim(d,false)?.id,second);Ok(())}).unwrap();
 }
 #[test]fn active_controls_are_exact_id_revision_and_phase_guarded_and_pause_snapshot_is_immediate(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=&s.items[0].id;let before=fs::read(&q.path).unwrap();
  for (target,rev) in [(&s.items[1].id,s.revision),(id,s.revision-1)]{assert!(q.pause_active(&vault,target,rev,true).is_err());assert_eq!(fs::read(&q.path).unwrap(),before);}
  let paused=q.pause_active(&vault,id,s.revision,true).unwrap();let p=serde_json::to_value(paused.items[0].progress.as_ref().unwrap()).unwrap();assert_eq!(p["paused"],true);assert_eq!(p["bytesPerSecond"],0);assert_eq!(p["done"],40);
  let resumed=q.pause_active(&vault,id,paused.revision,false).unwrap();assert!(!resumed.items[0].progress.as_ref().unwrap().paused);
  crate::vault::queue_end_control(&vault);let before=fs::read(&q.path).unwrap();assert!(q.request_control(&vault,id,resumed.revision,Control::Cancel,None).is_err());assert_eq!(fs::read(&q.path).unwrap(),before);assert!(!vault.cancel.load(Ordering::SeqCst));
 }
 #[test]fn only_job_cannot_defer_and_cancel_removes_only_its_owned_request_after_stop(){
  let f=AuthQueueFixture::new();let q=QueueState::load(f.root.join("q.json"));let s=add(&q,"one");let id=s.items[0].id.clone();let s=q.transaction(|d,_,_|{d.items[0].status=Status::Preparing;Ok(())}).unwrap();
  let vault=crate::vault::queue_test_state();assert!(!s.items[0].can_defer);assert!(s.items[0].can_cancel);assert!(q.request_control(&vault,&id,s.revision,Control::Defer,None).is_err());assert!(!vault.cancel.load(Ordering::SeqCst));
  let stop=q.request_control(&vault,&id,s.revision,Control::Cancel,None).unwrap();assert!(stop.active_id.is_some());assert!(stop.items[0].control_pending==Some(Control::Cancel));
  let mut forgotten=0;q.transact(true,|d,_,_|{settle(d,&id,Err("stopped".into()),None,false,|e|{assert_eq!(e.id,id);forgotten+=1;Ok(())})?;Ok(())}).unwrap();assert_eq!(forgotten,1);assert!(q.get().unwrap().items.is_empty());
 }
 #[test]fn persisted_defer_resumes_in_place_while_persisted_cancel_cleans_metadata_without_starting_paused_queue(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=s.items[0].id.clone();let second=s.items[1].id.clone();
  q.request_control(&vault,&id,s.revision,Control::Defer,None).unwrap();drop(q);
  let restored=QueueState::load(f.root.join("q.json"));let s=restored.get().unwrap();assert_eq!(s.items[2].id,id);assert!(s.items[2].status==Status::Pending);assert!(restored.inner.lock().unwrap().disk.items[2].resume_saved);assert!(s.paused);assert!(!restored.eligible());
  restored.transaction(|d,_,_|{d.items[2].control=Some(Control::Cancel);Ok(())}).unwrap();assert!(restored.eligible());
  restored.transaction(|d,p,_|{assert!(*p);let entry=claim(d,*p)?;assert_eq!(entry.id,id);settle(d,&id,Err("cancel".into()),None,true,|_|Ok(()))?;Ok(())}).unwrap();
  let s=restored.get().unwrap();assert!(s.paused);assert_eq!(s.items[0].id,second);assert_eq!(s.items.len(),2);assert!(!restored.eligible());
 }
 #[test]fn cancellation_cleanup_failure_preserves_actionable_request_and_other_entries(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=s.items[0].id.clone();q.request_control(&vault,&id,s.revision,Control::Cancel,None).unwrap();let other=serde_json::to_vec(&q.inner.lock().unwrap().disk.items[1]).unwrap();
  let result=q.transact(true,|d,_,_|{settle(d,&id,Err("cancel".into()),None,true,|_|Err("fixture failure".into()))?;Ok(())}).unwrap();
  assert!(result.items[0].status==Status::NeedsAction);assert_eq!(result.items[0].error.as_deref(),Some(CANCEL_ERROR));assert_eq!(result.items[0].package_id,Some("b".repeat(40)));assert_eq!(serde_json::to_vec(&q.inner.lock().unwrap().disk.items[1]).unwrap(),other);assert!(!q.eligible());
 }
 #[test]fn package_id_publication_alone_cannot_authorize_forgetting_or_resuming_a_prior_manual_transfer(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=s.items[0].id.clone();
  let s=q.transaction(|d,_,_|{d.items[0].owned_transfer=false;Ok(())}).unwrap();
  q.request_control(&vault,&id,s.revision,Control::Defer,None).unwrap();
  q.transact(true,|d,_,_|{settle(d,&id,Err(PARTIAL.into()),None,true,forget_transfer)?;Ok(())}).unwrap();
  let e=q.inner.lock().unwrap().disk.items.iter().find(|e|e.id==id).unwrap().clone();assert!(!e.resume_saved);assert!(!e.owned_transfer);
  // This ID has no task file in this fixture. The ownership guard returns before
  // looking at any task store, preserving an indistinguishable manual transfer.
  assert!(forget_transfer(&e).is_ok());
 }
 #[test]fn legacy_schema_defaults_new_control_and_ownership_fields_without_starting_saved_work(){
  let f=AuthQueueFixture::new();let q=QueueState::load(f.root.join("q.json"));add(&q,"one");let mut value=serde_json::to_value(&q.inner.lock().unwrap().disk).unwrap();
  for entry in value["items"].as_array_mut().unwrap(){for field in ["control","resumeSaved","ownedTransfer","savedProgress"]{entry.as_object_mut().unwrap().remove(field);}}
  fs::write(&q.path,serde_json::to_vec(&value).unwrap()).unwrap();let restored=QueueState::load(q.path.clone());let i=restored.inner.lock().unwrap();assert!(i.error.is_none());assert!(!i.disk.items[0].owned_transfer);assert!(!i.disk.items[0].resume_saved);assert!(i.paused);
 }
 #[test]fn removing_defer_predecessors_while_stopping_pauses_instead_of_restarting_same_transfer(){
  let f=AuthQueueFixture::new();let(q,vault,s)=active_queue(f.root.join("q.json"));let id=s.items[0].id.clone();q.request_control(&vault,&id,s.revision,Control::Defer,None).unwrap();
  q.transaction(|d,_,_|{d.items.retain(|e|e.id==id);Ok(())}).unwrap();
  let s=q.transact(true,|d,p,u|{settle_ordered(d,p,u,&id,Err("stopped".into()),None,true,|_|panic!("Defer never forgets"))?;Ok(())}).unwrap();
  assert!(s.paused);assert!(s.items[0].status==Status::Pending);assert!(q.inner.lock().unwrap().user_paused);assert!(!q.eligible());
  q.transaction(|_,p,u|{*p=false;*u=false;Ok(())}).unwrap();assert!(q.eligible());
 }
 struct AuthQueueFixture{root:PathBuf,parent:PathBuf}
 impl AuthQueueFixture{fn new()->Self{let parent=std::env::temp_dir();let path=root();crate::vault::no_links(&path).unwrap();fs::create_dir(&path).unwrap();Self{root:path,parent}}}
 impl Drop for AuthQueueFixture{fn drop(&mut self){if self.root.parent()!=Some(self.parent.as_path())||crate::vault::no_links(&self.root).is_err(){return;}let Ok(entries)=fs::read_dir(&self.root)else{return};let entries=entries.collect::<Result<Vec<_>,_>>();let Ok(entries)=entries else{return};if entries.iter().any(|e|e.file_name()!="q.json"||!e.file_type().is_ok_and(|t|t.is_file())||crate::vault::no_links(&e.path()).is_err()){return;}for e in entries{let _=fs::remove_file(e.path());}let _=fs::remove_dir(&self.root);}}
 #[test]fn authentication_failure_keeps_partial_queue_binding_and_manual_resume_status(){
  let f=AuthQueueFixture::new();let q=interrupted_fixture(f.root.join("q.json"));
  let before=q.get().unwrap();let other=serde_json::to_vec(&q.inner.lock().unwrap().disk.items[1]).unwrap();
  let updated=q.transaction(|d,_,_|{failed_entry(&mut d.items[0],true,crate::epic::AUTH_REQUIRED);Ok(())}).unwrap();
  assert!(updated.items[0].status==Status::Interrupted);assert_eq!(updated.items[0].error.as_deref(),Some(crate::epic::AUTH_REQUIRED));
  assert_eq!(updated.items[0].package_id,before.items[0].package_id);assert_eq!(updated.items[0].id,before.items[0].id);assert_eq!(updated.items[0].requested_vault_path,before.items[0].requested_vault_path);
  assert_eq!(updated.paused,before.paused);assert!(q.inner.lock().unwrap().user_paused);assert!(!q.eligible());
  assert_eq!(serde_json::to_vec(&q.inner.lock().unwrap().disk.items[1]).unwrap(),other);
  let disk=QueueState::load(q.path.clone()).get().unwrap();assert_eq!(disk.items[0].package_id,before.items[0].package_id);assert_eq!(disk.items[0].error.as_deref(),Some(crate::epic::AUTH_REQUIRED));
 }
 #[test]fn authentication_failure_without_partial_stays_actionable_and_does_not_create_tasks(){
  let f=AuthQueueFixture::new();let q=QueueState::load(f.root.join("q.json"));let initial=add(&q,"one");
  let updated=q.transaction(|d,_,_|{failed_entry(&mut d.items[0],false,crate::epic::AUTH_REQUIRED);Ok(())}).unwrap();
  assert!(updated.items[0].status==Status::NeedsAction);assert!(updated.items[0].package_id.is_none());assert_eq!(updated.items[0].error.as_deref(),Some(crate::epic::AUTH_REQUIRED));assert_eq!(updated.items[0].id,initial.items[0].id);assert_eq!(updated.items[0].requested_vault_path,initial.items[0].requested_vault_path);assert!(!q.eligible());assert_eq!(fs::read_dir(&f.root).unwrap().count(),1);
 }
 #[test]fn non_auth_queue_failure_does_not_offer_login_or_lose_saved_package(){
  let f=AuthQueueFixture::new();let q=interrupted_fixture(f.root.join("q.json"));let package=q.get().unwrap().items[0].package_id.clone();
  for error in ["CDN HTTP 403 signed URL","401 text without typed API status","Ошибка файловой системы: disk full"]{
   let updated=q.transaction(|d,_,_|{failed_entry(&mut d.items[0],true,error);Ok(())}).unwrap();
   assert!(updated.items[0].status==Status::Interrupted);assert_eq!(updated.items[0].error.as_deref(),Some(PARTIAL));assert_eq!(updated.items[0].package_id,package);assert!(!q.eligible());
  }
 }
 #[test]fn durable_requests_restart_paused_and_hide_native_account_binding(){
  let root=root();let path=root.join("queue.json");let q=QueueState::load(path.clone());assert!(q.get().unwrap().paused);
  let s=add(&q,"one");assert!(!s.paused);let id=s.items[0].id.clone();
  q.transaction(|d,_,_|{d.items[0].status=Status::Downloading;d.items[0].package_id=Some("b".repeat(40));Ok(())}).unwrap();
  let restored=QueueState::load(path.clone());let s=restored.get().unwrap();assert!(s.paused);assert!(s.active_id.is_none());assert_eq!(s.items[0].id,id);assert!(s.items[0].status==Status::Interrupted);assert_eq!(s.items[0].package_id,Some("b".repeat(40)));assert!(!restored.eligible());
  let ipc=serde_json::to_string(&s).unwrap();assert!(!ipc.contains("accountKey"));assert!(!ipc.contains(&"a".repeat(64)));assert!(!fs::read_to_string(path).unwrap().contains("https://"));fs::remove_dir_all(root).unwrap();
 }
 #[test]fn reorder_is_exact_pending_set_cas_and_cannot_move_active(){
  let root=root();let q=QueueState::load(root.join("q.json"));add(&q,"one");add(&q,"two");add(&q,"three");
  let s=q.transaction(|d,_,_|{d.items[0].status=Status::Downloading;Ok(())}).unwrap();let active=s.items[0].id.clone();let pending=vec![s.items[2].id.clone(),s.items[1].id.clone()];
  for ids in [vec![active.clone(),pending[0].clone()],vec![pending[0].clone(),pending[0].clone()],vec![pending[0].clone()],vec![uuid::Uuid::new_v4().to_string(),pending[0].clone()]]{assert!(q.transaction(|d,_,_|reorder(d,&ids,s.revision)).is_err());}
  let before=fs::read(&q.path).unwrap();assert!(q.transaction(|d,_,_|reorder(d,&pending,s.revision-1)).is_err());assert_eq!(fs::read(&q.path).unwrap(),before);
  let next=q.transaction(|d,_,_|reorder(d,&pending,s.revision)).unwrap();assert_eq!(next.items[0].id,active);assert_eq!(next.items[1].id,pending[0]);assert_eq!(next.revision,s.revision+1);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn concurrent_metadata_edits_allow_only_one_same_revision_claim(){
  let root=root();let q=std::sync::Arc::new(QueueState::load(root.join("q.json")));let s=add(&q,"one");let barrier=std::sync::Arc::new(std::sync::Barrier::new(3));let mut workers=vec![];
  for n in 0..2{let q=q.clone();let b=barrier.clone();let revision=s.revision;workers.push(std::thread::spawn(move||{b.wait();q.transaction(|d,p,u|{super::revision(d,revision)?;enqueue(d,p,*u,request(&format!("new-{n}")),"X:/Fixture/Vault".into(),"a".repeat(64))}).is_ok()}));}barrier.wait();let outcomes=workers.into_iter().map(|w|w.join().unwrap()).collect::<Vec<_>>();assert_eq!(outcomes.into_iter().filter(|p|*p).count(),1);let loaded=QueueState::load(q.path.clone());assert_eq!(loaded.get().unwrap().items.len(),2);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn enqueue_is_metadata_only_respects_user_pause_and_rejects_duplicates(){
  let root=root();let q=QueueState::load(root.join("q.json"));q.transaction(|_,p,u|{*p=true;*u=true;Ok(())}).unwrap();let s=add(&q,"one");assert!(s.paused);assert_eq!(fs::read_dir(&root).unwrap().count(),1);let before=fs::read(&q.path).unwrap();assert!(q.transaction(|d,p,u|enqueue(d,p,*u,request("one"),"X:/Fixture/Vault".into(),"a".repeat(64))).is_err());assert_eq!(fs::read(&q.path).unwrap(),before);assert!(s.items[0].package_id.is_none());fs::remove_dir_all(root).unwrap();
 }
 #[test]fn corrupt_or_oversized_queue_is_not_silently_replaced(){
  let root=root();fs::create_dir_all(&root).unwrap();let path=root.join("q.json");
  for bytes in [b"{bad queue".to_vec(),vec![b'x';MAX_BYTES as usize+1]]{fs::write(&path,&bytes).unwrap();let q=QueueState::load(path.clone());assert!(q.get().unwrap().error.is_some());assert!(q.transaction(|_,_,_|Ok(())).is_err());assert_eq!(fs::read(&path).unwrap(),bytes);}fs::remove_dir_all(root).unwrap();
 }
 #[test]fn stopping_blocks_dispatch_but_allows_active_interruption_receipt(){
  let root=root();let q=QueueState::load(root.join("q.json"));add(&q,"one");assert!(q.eligible());q.transaction(|d,_,_|{d.items[0].status=Status::Downloading;Ok(())}).unwrap();{let mut i=q.inner.lock().unwrap();i.stopping=true;i.paused=true;}
  assert!(!q.eligible());assert!(q.transaction(|_,_,_|Ok(())).is_err());q.transact(true,|d,_,_|{d.items[0].status=Status::Interrupted;Ok(())}).unwrap();assert!(QueueState::load(q.path.clone()).get().unwrap().items[0].status==Status::Interrupted);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn queue_request_rejects_urls_paths_control_characters_and_excessive_metadata(){
  let r=request("one");assert!(valid_request(&r));for asset in ["../escape","https://example.com/token","a\\b",""]{let mut r=r.clone();r.asset_id=asset.into();assert!(!valid_request(&r));}let mut long=r.clone();long.title="x".repeat(1025);assert!(!valid_request(&long));let mut control=r;control.title="name\nsecret".into();assert!(!valid_request(&control));
 }
 #[test]fn enqueue_after_restart_cannot_start_restored_work_without_explicit_start(){
  let root=root();let path=root.join("q.json");let q=QueueState::load(path.clone());add(&q,"old");drop(q);let restored=QueueState::load(path);let s=add(&restored,"new");assert!(s.paused);assert!(!restored.eligible());let s=restored.transaction(|_,paused,user_paused|{*paused=false;*user_paused=false;Ok(())}).unwrap();assert!(!s.paused);assert!(restored.eligible());fs::remove_dir_all(root).unwrap();
 }
 fn interrupted_fixture(path:PathBuf)->QueueState{
  let q=QueueState::load(path.clone());add(&q,"one");add(&q,"two");
  q.transaction(|d,_,_|{d.items[0].status=Status::Downloading;d.items[0].package_id=Some("b".repeat(40));Ok(())}).unwrap();drop(q);QueueState::load(path)
 }
 #[tokio::test]async fn manual_success_after_restart_preserves_explicit_start_and_unblocks_next_request(){
  let root=root();let vault=PathBuf::from("X:/Fixture/Vault");let package="b".repeat(40);
  for started in [false,true]{
   let q=std::sync::Arc::new(interrupted_fixture(root.join(format!("{started}.json"))));let initial=q.get().unwrap();let next=initial.items[1].id.clone();
   assert!(initial.items[0].status==Status::Interrupted);assert!(initial.paused);assert!(!q.eligible());
   if started{q.transaction(|_,p,u|{*p=false;*u=false;Ok(())}).unwrap();assert!(!q.eligible());}
   let worker=q.clone();let target=vault.join(&package);let vault=vault.clone();let package=package.clone();
   let completed=tokio::time::timeout(std::time::Duration::from_secs(2),tokio::spawn(async move{worker.manual_completed(&package,"asset-one",&target,&vault).unwrap()})).await.unwrap().unwrap();
   assert_eq!(completed.items.len(),1);assert_eq!(completed.items[0].id,next);assert_eq!(completed.paused,!started);assert_eq!(q.eligible(),started);assert_eq!(q.inner.lock().unwrap().user_paused,!started);
   let durable=QueueState::load(q.path.clone()).get().unwrap();assert_eq!(durable.items.len(),1);assert_eq!(durable.items[0].id,next);
   if started{q.transaction(|d,_,_|{assert!(d.items[0].status==Status::Pending);d.items[0].status=Status::Preparing;Ok(())}).unwrap();assert_eq!(q.get().unwrap().active_id,Some(next));}
  }fs::remove_dir_all(root).unwrap();
 }
 #[test]fn manual_completion_is_exact_scope_and_never_removes_active_or_pending_requests(){
  let root=root();let q=interrupted_fixture(root.join("q.json"));let vault=PathBuf::from("X:/Fixture/Vault");let id="b".repeat(40);let target=vault.join(&id);let before=fs::read(&q.path).unwrap();let revision=q.get().unwrap().revision;
  for (package,asset,path,scope) in [("c".repeat(40),"asset-one",vault.join("c".repeat(40)),vault.clone()),(id.clone(),"asset-other",target.clone(),vault.clone()),(id.clone(),"asset-one",PathBuf::from("X:/Fixture/Other").join(&id),vault.clone()),(id.clone(),"asset-one",target.clone(),PathBuf::from("X:/Fixture/Other"))]{assert!(q.manual_completed(&package,asset,&path,&scope).is_none());}
  assert_eq!(fs::read(&q.path).unwrap(),before);assert_eq!(q.get().unwrap().revision,revision);
  for status in [Status::Pending,Status::Preparing,Status::Downloading]{q.transaction(|d,_,_|{d.items[0].status=status;Ok(())}).unwrap();let before=fs::read(&q.path).unwrap();assert!(q.manual_completed(&id,"asset-one",&target,&vault).is_none());assert_eq!(fs::read(&q.path).unwrap(),before);}
  q.transaction(|d,_,_|{d.items[0].status=Status::NeedsAction;Ok(())}).unwrap();let other=serde_json::to_vec(&q.inner.lock().unwrap().disk.items[1]).unwrap();
  let done=q.manual_completed(&id,"asset-one",&target,&vault).unwrap();assert_eq!(done.items.len(),1);assert_eq!(serde_json::to_vec(&q.inner.lock().unwrap().disk.items[0]).unwrap(),other);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn unmatched_and_repeated_manual_success_do_not_create_or_rewrite_queue_files(){
  let root=root();let path=root.join("q.json");let q=QueueState::load(path.clone());let vault=PathBuf::from("X:/Fixture/Vault");let id="b".repeat(40);let target=vault.join(&id);
  assert!(q.manual_completed(&id,"asset-one",&target,&vault).is_none());assert!(!path.exists());assert_eq!(q.get().unwrap().revision,0);
  drop(q);let q=interrupted_fixture(path);q.manual_completed(&id,"asset-one",&target,&vault).unwrap();let before=fs::read(&q.path).unwrap();let revision=q.get().unwrap().revision;
  assert!(q.manual_completed(&id,"asset-one",&target,&vault).is_none());assert_eq!(q.get().unwrap().revision,revision);assert_eq!(fs::read(&q.path).unwrap(),before);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn manual_success_while_stopping_is_durable_without_restarting_dispatch(){
  let root=root();let q=interrupted_fixture(root.join("q.json"));let id="b".repeat(40);let vault=PathBuf::from("X:/Fixture/Vault");
  {let mut i=q.inner.lock().unwrap();i.stopping=true;i.paused=true;i.user_paused=false;}
  let s=q.manual_completed(&id,"asset-one",&vault.join(&id),&vault).unwrap();assert!(s.paused);assert_eq!(s.items.len(),1);assert!(!q.eligible());
  {let i=q.inner.lock().unwrap();assert!(i.stopping);assert!(!i.user_paused);}assert_eq!(QueueState::load(q.path.clone()).get().unwrap().items.len(),1);fs::remove_dir_all(root).unwrap();
 }
 #[cfg(windows)]#[test]fn failed_manual_completion_write_preserves_saved_queue_and_surfaces_paused_error(){
  use std::os::windows::fs::OpenOptionsExt;
  let root=root();let q=interrupted_fixture(root.join("q.json"));let before=fs::read(&q.path).unwrap();let initial=q.get().unwrap();let vault=PathBuf::from("X:/Fixture/Vault");let id="b".repeat(40);
  // An actual Windows sharing lock makes the atomic replacement fail while the
  // original durable bytes remain readable and unchanged.
  let held=fs::OpenOptions::new().read(true).share_mode(1).open(&q.path).unwrap();
  let s=q.manual_completed(&id,"asset-one",&vault.join(&id),&vault).unwrap();assert!(s.paused);assert_eq!(s.error.as_deref(),Some(SAVE_ERROR));assert_eq!(s.items.len(),2);assert_eq!(s.revision,initial.revision);assert_eq!(fs::read(&q.path).unwrap(),before);assert!(!q.eligible());assert!(q.inner.lock().unwrap().user_paused);
  drop(held);assert_eq!(QueueState::load(q.path.clone()).get().unwrap().items.len(),2);fs::remove_dir_all(root).unwrap();
 }
}

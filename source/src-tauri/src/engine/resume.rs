use super::*;
use super::install::{Operation,stage_root};
use std::io::{BufReader,BufWriter,Read,Write};
use flate2::{Compression,read::ZlibDecoder,write::ZlibEncoder};
use std::sync::LazyLock;
use std::time::{Duration,Instant};
pub(super) const SUSPENDED:&str="Установка сохранена для продолжения";
// egs-api's Deserialize expects Epic's decimal blobs, whereas its Serialize
// writes SHA1 hex strings and numeric offsets. Decode our persisted format
// explicitly, including snapshots already written by beta.5.
pub(crate) mod snapshot_files {
 use super::*;
 use egs_api::api::types::download_manifest::FileChunkPart;
 #[derive(Deserialize)]#[serde(rename_all="PascalCase")]
 struct File {filename:String,file_hash:String,file_chunk_parts:Vec<Part>}
 #[derive(Deserialize)]#[serde(rename_all="PascalCase")]
 struct Part {guid:String,link:Option<reqwest::Url>,offset:u128,size:u128}
 pub fn serialize<S:serde::Serializer>(files:&BTreeMap<String,FileManifestList>,serializer:S)->Result<S::Ok,S::Error>{files.serialize(serializer)}
 pub fn deserialize<'de,D:serde::Deserializer<'de>>(deserializer:D)->Result<BTreeMap<String,FileManifestList>,D::Error>{
  let saved=BTreeMap::<String,File>::deserialize(deserializer)?;
  let mut result=BTreeMap::new();
  for(name,f)in saved {
   if name!=f.filename || f.file_hash.len()!=40 || !f.file_hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err(serde::de::Error::custom("Invalid saved file hash or name"));}
   result.insert(name,FileManifestList{filename:f.filename,file_hash:f.file_hash,file_chunk_parts:f.file_chunk_parts.into_iter().map(|p|FileChunkPart{guid:p.guid,link:p.link,offset:p.offset,size:p.size}).collect()});
  }Ok(result)
 }
}
pub(super) fn check_suspend(flag:Option<&AtomicBool>)->Result<(),String>{if flag.is_some_and(|f|f.load(Ordering::SeqCst)){Err(SUSPENDED.into())}else{Ok(())}}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Task{pub id:String,pub path:String,pub build:String,pub phase:String,pub percent:u64,#[serde(default)]pub can_recover:bool,stage:PathBuf}
fn directory()->PathBuf{crate::storage::settings().join("EngineTasks")}
fn task_path(base:&Path,id:&str,extension:&str)->Result<PathBuf,String>{uuid::Uuid::parse_str(id).map_err(|_|"Некорректная задача установки")?;let p=base.join(format!("{id}.{extension}"));no_links(&p)?;Ok(p)}
pub(crate) fn write_compressed<T:Serialize>(path:&Path,value:&T)->Result<(),String>{
 no_links(path)?;fs::create_dir_all(path.parent().ok_or("Нет папки задачи")?).map_err(io)?;
 let temp=path.with_extension(format!("{}.tmp",uuid::Uuid::new_v4()));
 let result=(||{let file=fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(io)?;let mut encoder=ZlibEncoder::new(BufWriter::with_capacity(256*1024,file),Compression::fast());serde_json::to_writer(&mut encoder,value).map_err(|e|e.to_string())?;let mut writer=encoder.finish().map_err(io)?;writer.flush().map_err(io)?;writer.get_ref().sync_all().map_err(io)?;drop(writer);fs::rename(&temp,path).map_err(io)})();
 if result.is_err(){let _=fs::remove_file(&temp);}result
}
pub(crate) fn read_compressed<T:serde::de::DeserializeOwned>(path:&Path)->Result<T,String>{no_links(path)?;let file=fs::File::open(path).map_err(io)?;let decoder=ZlibDecoder::new(BufReader::with_capacity(256*1024,file));serde_json::from_reader(BufReader::with_capacity(256*1024,decoder.take(2*1024*1024*1024))).map_err(|_|"Повреждена сохранённая задача установки".into())}
pub(super) fn validate_stage(plan:&Plan,stage:&Path)->Result<(),String>{
 let root=target(&plan.info.path)?;no_links(stage)?;
 let fingerprint=super::install::digest(format!("{}\n{}",plan.info.path.to_lowercase(),plan.info.build).as_bytes());
 let name=format!(".um-engine-{}",&fingerprint[..20]);
 let actual=stage.file_name().and_then(|s|s.to_str()).ok_or("Некорректная рабочая папка")?;
 if stage.parent()!=root.parent()||(actual!=name&&actual!=format!("{name}-{}",plan.info.id)){return Err("Рабочая папка не соответствует задаче".into());}Ok(())
}
pub(super) fn save(plan:&Plan)->Result<(),String>{save_at(&directory(),plan)}
pub(super) fn save_at(base:&Path,plan:&Plan)->Result<(),String>{
 if list_at(base).iter().any(|t|t.id!=plan.info.id&&same_installation_path(&t.path,&plan.info.path)){return Err("В этой папке есть сохранённая установка. Нажмите Продолжить установку.".into());}
 let stage=stage_root(plan)?;validate_stage(plan,&stage)?;
 let snapshot=task_path(base,&plan.info.id,"zlib")?;
 if !snapshot.exists(){write_compressed(&snapshot,plan)?;}
 let meta=task_path(base,&plan.info.id,"json")?;
 if !meta.exists(){let task=Task{id:plan.info.id.clone(),path:plan.info.path.clone(),build:plan.info.build.clone(),stage,phase:"Установка сохранена для продолжения".into(),percent:0,can_recover:false};super::install::atomic(&meta,&serde_json::to_vec(&task).map_err(|e|e.to_string())?)?;}Ok(())
}
pub(super) fn save_operations(stage:&Path,ops:&[Operation])->Result<(),String>{let p=stage.join("resume-operations.zlib");if !p.exists(){write_compressed(&p,&ops)?;}Ok(())}
pub(super) fn load_operations(stage:&Path)->Result<Option<Vec<Operation>>,String>{
 let p=stage.join("resume-operations.zlib");no_links(&p)?;
 let ops:Option<Vec<Operation>>=if p.exists(){Some(read_compressed(&p)?)}else if commit_applying(stage)?{
   #[derive(Deserialize)]struct Saved{operations:Vec<Operation>}
   let j:Saved=serde_json::from_reader(BufReader::new(fs::File::open(stage.join("journal.json")).map_err(io)?)).map_err(|_|"Повреждён журнал установки")?;Some(j.operations)
 }else{None};
 if let Some(ops)=&ops{for op in ops{relative(&op.name)?;}}Ok(ops)
}
fn journal_status(stage:&Path)->Result<Option<String>,String>{let p=stage.join("journal.json");no_links(&p)?;if !p.exists(){return Ok(None);}let v:serde_json::Value=serde_json::from_reader(BufReader::new(fs::File::open(p).map_err(io)?)).map_err(|_|"Повреждён журнал установки")?;Ok(v["status"].as_str().map(str::to_owned))}
pub(super) fn commit_complete(stage:&Path)->Result<bool,String>{Ok(journal_status(stage)?.as_deref()==Some("complete"))}
pub(super) fn commit_applying(stage:&Path)->Result<bool,String>{Ok(journal_status(stage)?.as_deref()==Some("applying"))}
pub(super) fn remove(id:&str)->Result<(),String>{for ext in ["json","zlib"]{let p=task_path(&directory(),id,ext)?;if p.exists(){fs::remove_file(p).map_err(io)?;}}Ok(())}
static REPORTED:LazyLock<Mutex<BTreeMap<String,(String,Instant)>>>=LazyLock::new(||Mutex::new(BTreeMap::new()));
fn persist_report(path:&Path,phase:&str,percent:u64)->Result<(),String>{
 no_links(path)?;
 let bytes=fs::read(path).map_err(io)?;
 let mut task:Task=serde_json::from_slice(&bytes).map_err(|_|"Повреждена задача установки".to_string())?;
 // A heartbeat inside a large-file read does not change durable recovery state.
 if task.phase==phase && task.percent==percent{return Ok(());}
 task.phase=phase.into();task.percent=percent;
 super::install::atomic(path,&serde_json::to_vec(&task).map_err(|e|e.to_string())?)
}
pub(super) fn report(id:&str,phase:&str,done:u64,total:u64){
 let Ok(mut reported)=REPORTED.lock()else{return;};if reported.get(id).is_some_and(|(p,t)|p==phase&&t.elapsed()<Duration::from_secs(2)){return;}
 let Ok(p)=task_path(&directory(),id,"json")else{return;};
 let percent=if total>0{done.saturating_mul(100)/total}else{0};
 if persist_report(&p,phase,percent).is_ok(){reported.insert(id.into(),(phase.into(),Instant::now()));}
}
#[tauri::command]
pub fn engine_resume_tasks()->Vec<Task>{let mut tasks=list_at(&directory());for task in &mut tasks{task.can_recover=Path::new(&task.path).join(".um-engine-update.json").exists();}tasks}
#[tauri::command]
pub async fn engine_resume_forget(state:tauri::State<'_,EngineState>,id:String,expected_path:String)->Result<(),String>{
    let _update_operation=crate::app_updates::operation()?;
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции".into());}let _busy=super::install::Busy(&state.active);
 tauri::async_runtime::spawn_blocking(move||{
  let task:Task=serde_json::from_slice(&fs::read(task_path(&directory(),&id,"json")?).map_err(io)?).map_err(|_|"Повреждена задача установки")?;
  if task.id!=id||!same_installation_path(&task.path,&expected_path){return Err("Задача изменилась. Обновите список.".into());}
  let root=target(&task.path)?;let _lock=super::install::lock_installation(&root)?;
  if root.join(".um-engine-update.json").exists(){return Err("Сначала завершите или восстановите обновление движка.".into());}
  // Forget only metadata. Engine folders, rollback backups and downloaded blocks stay intact.
  remove(&id)
 }).await.map_err(|e|e.to_string())?
}
fn list_at(base:&Path)->Vec<Task>{
 if no_links(base).is_err(){return vec![];}fs::read_dir(base).into_iter().flatten().flatten().filter_map(|e|{let p=e.path();if p.extension()?.to_str()?!="json"||no_links(&p).is_err(){return None;}let t:Task=serde_json::from_slice(&fs::read(&p).ok()?).ok()?;if p.file_stem()?.to_str()?!=t.id||task_path(base,&t.id,"zlib").ok()?.is_file()==false{return None;}let root=target(&t.path).ok()?;if t.stage.parent()!=root.parent()||no_links(&t.stage).is_err(){return None;}Some(t)}).collect()
}
fn resume_vault_lease(vault:&crate::vault::VaultState)->Result<super::install::Busy<'_>,String>{
 vault.active.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).map_err(|_|"Дождитесь текущей операции с хранилищем")?;
 Ok(super::install::Busy(&vault.active))
}
pub(super) fn load_at(base:&Path,id:&str)->Result<Plan,String>{
 let task:Task=serde_json::from_slice(&fs::read(task_path(base,id,"json")?).map_err(io)?).map_err(|_|"Повреждена задача установки")?;
 let mut plan:Plan=read_compressed(&task_path(base,id,"zlib")?)?;
 if plan.info.id!=id||plan.info.path!=task.path||plan.info.build!=task.build{return Err("Задача не соответствует сохранённой установке".into());}
 validate_stage(&plan,&task.stage)?;plan.resume_stage=Some(task.stage);Ok(plan)
}
#[tauri::command]
pub async fn engine_resume_prepare(state:tauri::State<'_,EngineState>,vault:tauri::State<'_,crate::vault::VaultState>,id:String)->Result<Preview,String>{
    let _update_operation=crate::app_updates::operation()?;
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции".into());}
 let _busy=super::install::Busy(&state.active);
 let _vault=resume_vault_lease(&vault)?;
 let mut plan=tauri::async_runtime::spawn_blocking(move||load_at(&directory(),&id)).await.map_err(|e|format!("Не удалось прочитать сохранённую задачу: {e}"))??;
 let mut manifest=parse(&plan.raw)?;if manifest.app_name_string!=plan.info.app_name||manifest.build_version_string!=plan.info.build{return Err("Манифест не соответствует задаче".into());}
 manifest.custom_fields.get_or_insert_with(Default::default).insert("SourceURL".into(),plan.source_base.trim_end_matches('/').into());
 let verified=manifest.files();
 for (name,f) in &plan.files{if verified.get(name)!=Some(f){return Err("Файлы задачи не совпадают с манифестом".into());}relative(name)?;if f.filename!=*name||f.file_hash.len()!=40{return Err("Некорректные файлы задачи".into());}for part in &f.file_chunk_parts{if part.link.as_ref().is_none_or(|u|u.scheme()!="https"){return Err("Некорректный адрес блока".into());}}}
 if let Some((path,item))=&plan.item{if !items().iter().any(|(p,_)|p==path)||!same_installation_path(item["InstallLocation"].as_str().unwrap_or(""),&plan.info.path){return Err("Не совпадает запись Epic в задаче".into());}}
 plan.info.free_bytes=free(&target(&plan.info.path)?)?;let info=plan.info.clone();*state.plan.lock().map_err(|_|"Ошибка состояния")?=Some(plan);Ok(info)
}
#[cfg(test)]mod tests{
 use super::*;
 #[cfg(windows)]
 #[test]fn unchanged_read_heartbeat_does_not_rewrite_a_locked_checkpoint(){
  use std::os::windows::fs::OpenOptionsExt;
  let parent=std::env::temp_dir();no_links(&parent).unwrap();
  let id=uuid::Uuid::new_v4().to_string();let base=parent.join(format!("um-report-{id}"));
  fs::create_dir(&base).unwrap();
  struct Owned{base:PathBuf,parent:PathBuf,id:String}
  impl Drop for Owned{fn drop(&mut self){
   if self.base.parent()!=Some(self.parent.as_path())||self.base.file_name().and_then(|s|s.to_str())!=Some(format!("um-report-{}",self.id).as_str())||no_links(&self.base).is_err(){return;}
   let p=self.base.join("task.json");if no_links(&p).is_ok()&&p.is_file(){let _=fs::remove_file(p);}
   if fs::read_dir(&self.base).is_ok_and(|mut entries|entries.next().is_none()){let _=fs::remove_dir(&self.base);}
  }}
  let _owned=Owned{base:base.clone(),parent,id:id.clone()};let path=base.join("task.json");
  let task=Task{id,path:"X:/Fixture/Engine".into(),build:"synthetic".into(),phase:"Устанавливаем движок".into(),percent:16,can_recover:true,stage:PathBuf::from("X:/Fixture/Stage")};
  let original=serde_json::to_vec(&task).unwrap();fs::write(&path,&original).unwrap();
  let locked=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
  // A needless atomic replacement would fail while this read handle denies DELETE.
  assert!(persist_report(&path,"Устанавливаем движок",16).is_ok());
  assert_eq!(fs::read(&path).unwrap(),original);
  assert!(persist_report(&path,"Устанавливаем движок",17).is_err());
  assert_eq!(fs::read(&path).unwrap(),original);drop(locked);
  persist_report(&path,"Устанавливаем движок",17).unwrap();
  let changed:Task=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
  assert_eq!(changed.percent,17);assert!(changed.can_recover);
 }
 #[test]fn saved_and_legacy_preparation_share_the_vault_heavy_operation_lease(){
  let vault=crate::vault::VaultState::default();vault.active.store(true,Ordering::SeqCst);assert!(resume_vault_lease(&vault).is_err());assert!(vault.active.load(Ordering::SeqCst));vault.active.store(false,Ordering::SeqCst);
  let saved=resume_vault_lease(&vault).unwrap();assert!(vault.active.load(Ordering::SeqCst));assert!(resume_vault_lease(&vault).is_err());drop(saved);assert!(!vault.active.load(Ordering::SeqCst));let legacy=resume_vault_lease(&vault).unwrap();drop(legacy);assert!(!vault.active.load(Ordering::SeqCst));
 }
 #[test]fn compressed_snapshot_is_atomic_and_round_trips(){let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());let p=base.join("snapshot.z");write_compressed(&p,&vec![1u32,2,3]).unwrap();assert_eq!(read_compressed::<Vec<u32>>(&p).unwrap(),vec![1,2,3]);assert_eq!(fs::read_dir(&base).unwrap().count(),1);fs::remove_dir_all(base).unwrap();}
 #[test]fn task_ids_cannot_escape_settings(){assert!(task_path(Path::new("X:/Fixture/Test"),"../file","json").is_err());}
 #[test]fn real_hash_and_numeric_chunk_offsets_round_trip_without_epic_blob_decoder(){
  #[derive(Serialize,Deserialize)]struct Saved{#[serde(with="snapshot_files")]files:BTreeMap<String,FileManifestList>}
  let mut files=BTreeMap::new();files.insert("Engine/file".into(),FileManifestList{filename:"Engine/file".into(),file_hash:"a7ef12b3c4d509876543210fedcba01234567890".into(),file_chunk_parts:vec![egs_api::api::types::download_manifest::FileChunkPart{guid:"guid".into(),link:Some(reqwest::Url::parse("https://example.com/chunk").unwrap()),offset:8192,size:1048576}]});
  let bytes=serde_json::to_vec(&Saved{files:files.clone()}).unwrap();let restored:Saved=serde_json::from_slice(&bytes).unwrap();assert_eq!(restored.files,files);
  let invalid=String::from_utf8(bytes).unwrap().replace("a7ef12b3c4d509876543210fedcba01234567890","invalid-hash");assert!(serde_json::from_str::<Saved>(&invalid).is_err());
 }
}

#[tauri::command]
pub async fn engine_resume_legacy_prepare(state:tauri::State<'_,EngineState>,vault:tauri::State<'_,crate::vault::VaultState>,path:String)->Result<Preview,String>{
    let _update_operation=crate::app_updates::operation()?;
 if let Some(task)=engine_resume_tasks().into_iter().find(|t|same_installation_path(&t.path,&path)){return engine_resume_prepare(state,vault,task.id).await;}
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции".into());}
 let _busy=super::install::Busy(&state.active);
 let _vault=resume_vault_lease(&vault)?;
 let plan=tauri::async_runtime::spawn_blocking(move||{let plan=super::install::legacy_plan(&path)?;save(&plan)?;Ok::<_,String>(plan)}).await.map_err(|_|"Чтение сохранённой установки прервано")??;
 let info=plan.info.clone();*state.plan.lock().map_err(|_|"Ошибка состояния")?=Some(plan);Ok(info)
}

pub(crate) fn inspect_legacy(path:&str)->Result<serde_json::Value,String>{let plan=super::install::legacy_plan(path)?;Ok(serde_json::json!({"build":plan.info.build,"path":plan.info.path,"files":plan.files.len(),"components":plan.info.components.iter().filter(|c|c.selected).map(|c|c.tag.clone()).collect::<Vec<_>>(),"stage":plan.resume_stage}))}
#[cfg(any(debug_assertions,feature="engine-test-cli"))]
pub(crate) fn inspect_saved(base:&Path,id:&str)->Result<serde_json::Value,String>{
 let plan=load_at(base,id)?;let mut manifest=parse(&plan.raw)?;
 manifest.custom_fields.get_or_insert_with(Default::default).insert("SourceURL".into(),plan.source_base.trim_end_matches('/').into());
 let verified=manifest.files();if plan.files.iter().any(|(name,file)|verified.get(name)!=Some(file)){return Err("Файлы задачи не совпадают с манифестом".into());}
 Ok(serde_json::json!({"build":plan.info.build,"path":plan.info.path,"files":plan.files.len(),"oldFiles":plan.old.len(),"verified":true,"stage":plan.resume_stage}))
}

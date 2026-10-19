use super::*;
use std::sync::LazyLock;
pub(super) const STOPPED:&str="Загрузка остановлена. Готовые файлы сохранены для повторной попытки";
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase")]
pub struct Intent {pub mode:String,pub project:String,pub project_name:String,pub engine_version:String,#[serde(default)]pub update_id:Option<String>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Task {pub id:String,pub asset_id:String,pub title:String,pub kind:String,pub selected_engine:String,pub path:String,pub phase:String,pub done:u64,pub total:u64,pub files_done:usize,pub files_total:usize,#[serde(default)]pub intent:Option<Intent>}
fn directory()->PathBuf{crate::storage::settings().join("VaultTasks")}
fn owned(base:&Path,id:&str,extension:&str)->Result<PathBuf,String>{if !valid_hash(id){return Err("Некорректная задача Vault".into());}let p=base.join(format!("{id}.{extension}"));no_links(&p)?;Ok(p)}
fn validate(plan:&Plan)->Result<(),String>{
 let expected=crate::storage::vault().join(&plan.info.id);no_links(&expected)?;
 if !valid_hash(&plan.info.id)||Path::new(&plan.info.path)!=expected {return Err("Хранилище изменено. Верните исходную папку Vault для продолжения этой загрузки.".into());}
 if plan.files.len()!=plan.info.files||plan.files.len()!=plan.info.hashes.len(){return Err("Повреждён список файлов задачи Vault".into());}
 for(name,f)in &plan.files {
  relative(name)?;if name.eq_ignore_ascii_case("package.um.json")||name!=&f.filename||plan.info.hashes.get(name)!=Some(&f.file_hash.to_ascii_lowercase()){return Err("Повреждены контрольные суммы задачи Vault".into());}
  for p in &f.file_chunk_parts{if p.offset.checked_add(p.size).is_none_or(|v|v>CHUNK_LIMIT as u128)||p.link.as_ref().is_none_or(|u|u.scheme()!="https"){return Err("Некорректный блок задачи Vault".into());}}
 }Ok(())
}
pub(super) fn save(plan:&Plan)->Result<(),String>{validate(plan)?;save_at(&directory(),plan)}
fn save_at(base:&Path,plan:&Plan)->Result<(),String>{
 crate::engine::resume::write_compressed(&owned(base,&plan.info.id,"zlib")?,plan)?;
 let task=Task{id:plan.info.id.clone(),asset_id:plan.info.asset_id.clone(),title:plan.info.title.clone(),kind:plan.info.kind.clone(),selected_engine:plan.info.selected_engine.clone(),path:plan.info.path.clone(),phase:"Готова к продолжению".into(),done:0,total:plan.info.bytes,files_done:0,files_total:plan.info.files,intent:plan.intent.clone()};
 crate::storage::atomic(&owned(base,&plan.info.id,"json")?,&serde_json::to_vec(&task).map_err(|e|e.to_string())?)
}
fn load_at(base:&Path,id:&str)->Result<Plan,String>{let plan:Plan=crate::engine::resume::read_compressed(&owned(base,id,"zlib")?)?;if plan.info.id!=id{return Err("Задача Vault не совпадает с записью".into());}Ok(plan)}
#[tauri::command]
pub fn vault_resume_tasks()->Vec<Task>{
 let base=directory();if no_links(&base).is_err(){return vec![];}
 fs::read_dir(&base).into_iter().flatten().flatten().filter_map(|e|{let p=e.path();if p.extension()?.to_str()?!="json"||no_links(&p).is_err(){return None;}let t:Task=serde_json::from_slice(&fs::read(&p).ok()?).ok()?;if p.file_stem()?.to_str()?!=t.id||!owned(&base,&t.id,"zlib").ok()?.is_file(){return None;}Some(t)}).collect()
}
#[tauri::command]
pub async fn vault_resume_prepare(state:tauri::State<'_,VaultState>,id:String)->Result<Package,String>{
    let _update_operation=crate::app_updates::operation()?;
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей загрузки".into());}let _busy=Busy(&state.active);
 let plan=tauri::async_runtime::spawn_blocking(move||{let p=load_at(&directory(),&id)?;validate(&p)?;Ok::<_,String>(p)}).await.map_err(|e|e.to_string())??;
 let info=plan.info.clone();*state.plan.lock().map_err(|_|"Ошибка состояния загрузки")?=Some(plan);Ok(info)
}
pub(super) fn remove(id:&str)->Result<(),String>{for ext in ["json","zlib"]{let p=owned(&directory(),id,ext)?;if p.exists(){fs::remove_file(p).map_err(io_error)?;}}Ok(())}
#[tauri::command]
pub fn vault_resume_forget(state:tauri::State<'_,VaultState>,id:String)->Result<(),String>{
 if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей загрузки".into());}let _busy=Busy(&state.active);
 if state.updating.load(Ordering::SeqCst){return Err("Дождитесь текущей операции".into());}remove(&id)
}
static REPORTED:LazyLock<Mutex<BTreeMap<String,Instant>>>=LazyLock::new(||Mutex::new(BTreeMap::new()));
fn report(state:&VaultState,p:&Progress){
 let id=state.plan.lock().ok().and_then(|v|v.as_ref().map(|p|p.info.id.clone()));let Some(id)=id else{return};
 let Ok(mut samples)=REPORTED.lock()else{return};if !p.paused&&samples.get(&id).is_some_and(|t|t.elapsed()<Duration::from_secs(2)){return;}
 let Ok(path)=owned(&directory(),&id,"json")else{return};let Ok(bytes)=fs::read(&path)else{return};let Ok(mut task)=serde_json::from_slice::<Task>(&bytes)else{return};
 task.phase=if p.paused{"На паузе".into()}else{p.phase.clone()};task.done=p.done;task.total=p.total;task.files_done=p.files_done;task.files_total=p.files_total;
 if let Ok(bytes)=serde_json::to_vec(&task){if crate::storage::atomic(&path,&bytes).is_ok(){samples.insert(id,Instant::now());}}
}
pub(super) fn emit(state:&VaultState,mut p:Progress){
 p.paused=state.paused.load(Ordering::SeqCst);p.can_pause=state.pause_available.load(Ordering::SeqCst);
 if let Ok(mut sample)=state.rate.lock(){
  if p.paused{p.bytes_per_second=0;*sample=None;}else{
   let (time,bytes,rate)=sample.get_or_insert((Instant::now(),p.network_bytes,0));
   if time.elapsed()>=Duration::from_millis(250){*rate=((p.network_bytes.saturating_sub(*bytes) as f64)/time.elapsed().as_secs_f64()) as u64;*bytes=p.network_bytes;*time=Instant::now();}p.bytes_per_second=*rate;
  }
 }
 if let Ok(mut latest)=state.latest.lock(){*latest=Some(p.clone());}report(state,&p);
 if let Ok(channel)=state.channel.lock(){if let Some(c)=channel.as_ref(){let _=c.send(p);}}
}
fn status(state:&VaultState){let p=state.latest.lock().ok().and_then(|p|p.clone());if let Some(p)=p{emit(state,p);}}
pub(super) fn network(state:&VaultState,bytes:u64){if state.rate.lock().ok().and_then(|r|r.as_ref().map(|(t,_,_)|t.elapsed()<Duration::from_millis(250))).unwrap_or(false){return;}let p=state.latest.lock().ok().and_then(|p|p.clone());if let Some(mut p)=p{p.network_bytes=bytes;emit(state,p);}}
pub(super) async fn checkpoint(state:&VaultState)->Result<(),String>{
 loop{if state.cancel.load(Ordering::SeqCst){return Err(STOPPED.into());}if !state.paused.load(Ordering::SeqCst){return Ok(());}tokio::time::sleep(Duration::from_millis(100)).await;}
}
#[tauri::command]
pub fn vault_pause(state:tauri::State<'_,VaultState>,paused:bool)->Result<(),String>{
 if !state.active.load(Ordering::SeqCst)||!state.pause_available.load(Ordering::SeqCst){return Err("На этом этапе пауза недоступна.".into());}
 if paused{state.pause_generation.fetch_add(1,Ordering::SeqCst);}state.paused.store(paused,Ordering::SeqCst);status(&state);Ok(())
}
pub(super) struct Scope<'a>(pub &'a VaultState);
impl Drop for Scope<'_>{fn drop(&mut self){self.0.paused.store(false,Ordering::SeqCst);self.0.pause_available.store(false,Ordering::SeqCst);if let Ok(mut c)=self.0.channel.lock(){*c=None;}}}
pub(super) struct Temporary(pub PathBuf);
impl Drop for Temporary {fn drop(&mut self){let _=fs::remove_file(&self.0);}}

// A saved block is shared by all files referencing the same GUID. Partial HTTP
// bodies use Range; a server returning 200 causes a clean restart of that block.
pub(super) async fn chunk(client:&reqwest::Client,url:&reqwest::Url,guid:&str,base:&Path,state:&VaultState,network:&mut u64)->Result<Vec<u8>,String>{
 no_links(base)?;fs::create_dir_all(base).map_err(io_error)?;
 let key=digest(guid.as_bytes());let complete=base.join(format!("{key}.chunk"));let partial=base.join(format!("{key}.part"));no_links(&complete)?;no_links(&partial)?;
 if complete.exists(){if fs::metadata(&complete).map_err(io_error)?.len()>CHUNK_LIMIT as u64{return Err("Слишком большой блок загрузки".into());}let bytes=fs::read(&complete).map_err(io_error)?;if let Ok(data)=decode_chunk(&bytes){return Ok(data);}fs::remove_file(&complete).map_err(io_error)?;}
 loop {
  checkpoint(state).await?;
  let mut offset=fs::metadata(&partial).map(|m|m.len()).unwrap_or(0);
  if offset>CHUNK_LIMIT as u64{return Err("Слишком большой блок загрузки".into());}
  if offset>0 {let bytes=fs::read(&partial).map_err(io_error)?;if let Ok(data)=decode_chunk(&bytes){fs::rename(&partial,&complete).map_err(io_error)?;return Ok(data);}}
  let mut request=client.get(url.clone());if offset>0{request=request.header(reqwest::header::RANGE,format!("bytes={offset}-"));}
  // Time only active network waits, not intentional pauses or speed limiting.
  let future=tokio::time::timeout(Duration::from_secs(30),request.send());tokio::pin!(future);
  let response=loop{
   tokio::select!{value=&mut future=>break Some(value.map_err(|_|"Download timed out")?.map_err(|_|"Не удалось загрузить блок. Повторите загрузку")?),_=tokio::time::sleep(Duration::from_millis(100))=>{if state.cancel.load(Ordering::SeqCst){return Err(STOPPED.into());}if state.paused.load(Ordering::SeqCst){break None;}}}
  };let Some(mut response)=response else{continue};
  if response.status()==reqwest::StatusCode::PARTIAL_CONTENT {
   let expected=format!("bytes {offset}-");if !response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|h|h.to_str().ok()).is_some_and(|v|v.starts_with(&expected)){return Err("Сервер вернул неверный диапазон блока".into());}
  }else if response.status()==reqwest::StatusCode::OK{offset=0;}else{return Err(format!("Сервер загрузки вернул HTTP {}. Подготовьте загрузку заново",response.status().as_u16()));}
  let mut output=OpenOptions::new().create(true).write(true).truncate(offset==0).append(offset>0).open(&partial).map_err(io_error)?;
  let mut length=offset;let mut paused=false;
  loop{
   if state.cancel.load(Ordering::SeqCst){output.sync_all().map_err(io_error)?;return Err(STOPPED.into());}
   if state.paused.load(Ordering::SeqCst){paused=true;break;}
   let next=tokio::time::timeout(Duration::from_secs(30),response.chunk());tokio::pin!(next);
   let value=loop{tokio::select!{v=&mut next=>break Some(v.map_err(|_|"Download timed out")?.map_err(|_|"Загрузка прервана сетью. Повторите попытку")?),_=tokio::time::sleep(Duration::from_millis(100))=>{if state.cancel.load(Ordering::SeqCst){output.sync_all().map_err(io_error)?;return Err(STOPPED.into());}if state.paused.load(Ordering::SeqCst){break None;}}}};
   let Some(value)=value else{paused=true;break};let Some(bytes)=value else{break};
   length+=bytes.len() as u64;if length>CHUNK_LIMIT as u64{return Err("Слишком большой блок загрузки".into());}
   let generation=state.pause_generation.load(Ordering::SeqCst);let was_paused=state.paused.load(Ordering::SeqCst);
   output.write_all(&bytes).map_err(io_error)?;*network+=bytes.len() as u64;self::network(state,*network);
   crate::download::throttle_paused(bytes.len(),&state.cancel,&state.paused).await?;
   if was_paused||state.pause_generation.load(Ordering::SeqCst)!=generation{paused=true;break;}
  }
  output.sync_all().map_err(io_error)?;drop(output);drop(response);
  if paused{continue;}
  let bytes=fs::read(&partial).map_err(io_error)?;let data=decode_chunk(&bytes)?;fs::rename(&partial,&complete).map_err(io_error)?;return Ok(data);
 }
}
pub(super) fn cleanup(stage:&Path,id:&str)->Result<(),String>{
 no_links(stage)?;let chunks=stage.join("chunks");no_links(&chunks)?;
 if chunks.exists(){for e in fs::read_dir(&chunks).map_err(io_error)?{let p=e.map_err(io_error)?.path();no_links(&p)?;let name=p.file_stem().and_then(|n|n.to_str()).ok_or("Некорректный блок")?;if !valid_hash(name)||!matches!(p.extension().and_then(|e|e.to_str()),Some("chunk"|"part")){return Err("В папке загрузки есть неизвестные файлы".into());}fs::remove_file(p).map_err(io_error)?;}fs::remove_dir(chunks).map_err(io_error)?;}
 // Older versions could leave their UUID-named assembly temporaries behind.
 for entry in fs::read_dir(stage).map_err(io_error)?{let p=entry.map_err(io_error)?.path();let name=p.file_stem().and_then(|n|n.to_str()).unwrap_or("");if p.extension().and_then(|e|e.to_str())==Some("tmp")&&name.len()==32&&name.bytes().all(|b|b.is_ascii_hexdigit()){no_links(&p)?;fs::remove_file(p).map_err(io_error)?;}}
 fs::remove_dir(stage).map_err(io_error)?;if let Some(parent)=stage.parent(){let _=fs::remove_dir(parent);}remove(id)
}
#[cfg(test)]mod tests{
 use super::*;
 #[tokio::test]async fn paused_task_waits_and_cancel_unblocks_it(){let state=std::sync::Arc::new(VaultState::default());state.paused.store(true,Ordering::SeqCst);let s=state.clone();let task=tokio::spawn(async move{checkpoint(&s).await});tokio::time::sleep(Duration::from_millis(20)).await;assert!(!task.is_finished());state.cancel.store(true,Ordering::SeqCst);assert!(tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap().is_err());}
 #[test]fn chunk_guard_removes_only_its_temporary_file(){let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());fs::create_dir_all(&base).unwrap();fs::write(base.join("own.tmp"),b"partial").unwrap();fs::write(base.join("keep"),b"keep").unwrap();drop(Temporary(base.join("own.tmp")));assert!(!base.join("own.tmp").exists());assert_eq!(fs::read(base.join("keep")).unwrap(),b"keep");fs::remove_dir_all(base).unwrap();}
 fn fixture(root:&Path)->Plan{
  let id=digest(root.to_string_lossy().as_bytes());let hash=digest(b"real file data");let name="Content/Material.uasset".to_string();
  let file=FileManifestList{filename:name.clone(),file_hash:hash.clone(),file_chunk_parts:vec![egs_api::api::types::download_manifest::FileChunkPart{guid:"some-guid".into(),link:Some(reqwest::Url::parse("https://example.com/chunk").unwrap()),offset:4000,size:14}]};
  Plan{intent:Some(Intent{mode:"install".into(),project:"X:/Fixture/Game.uproject".into(),project_name:"Game".into(),engine_version:"5.8".into(),update_id:None}),info:Package{artifact_id:"artifact".into(),selected_engine:"5.8".into(),build_version:"build".into(),source:String::new(),asset_id:"asset-id".into(),id:id.clone(),title:"Material".into(),kind:"Asset".into(),engines:vec!["5.8".into()],bytes:14,download_bytes:4096,files:1,path:root.join(id).to_string_lossy().into_owned(),hashes:BTreeMap::from([(name.clone(),hash)])},files:BTreeMap::from([(name,file)])}
 }
 #[test]fn snapshot_survives_restart_with_exact_version_offsets_and_project_intent(){let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());let plan=fixture(&base);save_at(&base.join("tasks"),&plan).unwrap();let restored=load_at(&base.join("tasks"),&plan.info.id).unwrap();assert_eq!(restored.files,plan.files);assert_eq!(restored.info.selected_engine,"5.8");assert_eq!(restored.intent.unwrap().project,"X:/Fixture/Game.uproject");assert!(owned(&base,"../other","json").is_err());fs::remove_dir_all(base).unwrap();}
 fn encoded(data:&[u8])->Vec<u8>{let mut raw=vec![0u8;62];raw[0..4].copy_from_slice(&2986228386u32.to_le_bytes());raw[4..8].copy_from_slice(&2u32.to_le_bytes());raw[8..12].copy_from_slice(&62u32.to_le_bytes());raw[12..16].copy_from_slice(&(data.len()as u32).to_le_bytes());raw[41..61].copy_from_slice(&Sha1::digest(data));raw[61]=2;raw.extend_from_slice(data);raw}
 #[tokio::test]async fn interrupted_http_block_resumes_with_range_and_is_reused_without_network(){
  use std::net::{TcpListener,TcpStream};use std::sync::{Arc,atomic::AtomicUsize};
  fn headers(stream:&mut TcpStream)->String{let mut bytes=vec![];let mut b=[0];while !bytes.ends_with(b"\r\n\r\n"){stream.read_exact(&mut b).unwrap();bytes.push(b[0]);}String::from_utf8(bytes).unwrap()}
  let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());let data=vec![7u8;65536];let raw=encoded(&data);let half=12345;let server=TcpListener::bind("127.0.0.1:0").unwrap();let address=server.local_addr().unwrap();let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();let body=raw.clone();
  let worker=std::thread::spawn(move||{
   let(mut first,_)=server.accept().unwrap();first.set_read_timeout(Some(Duration::from_secs(5))).unwrap();let h=headers(&mut first);assert!(!h.to_lowercase().contains("range:"));count.fetch_add(1,Ordering::SeqCst);
   write!(first,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();first.write_all(&body[..half]).unwrap();first.flush().unwrap();let _=first.read(&mut[0u8;1]);drop(first);
   let(mut second,_)=server.accept().unwrap();second.set_read_timeout(Some(Duration::from_secs(5))).unwrap();let h=headers(&mut second);assert!(h.to_lowercase().contains(&format!("range: bytes={half}-")));count.fetch_add(1,Ordering::SeqCst);
   write!(second,"HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",body.len()-half,half,body.len()-1,body.len()).unwrap();second.write_all(&body[half..]).unwrap();
  });
  let state=Arc::new(VaultState::default());let s=state.clone();let dir=base.clone();let url=reqwest::Url::parse(&format!("http://{address}/chunk")).unwrap();let original=url.clone();
  let task=tokio::spawn(async move{let mut network=0;chunk(&reqwest::Client::new(),&original,"guid",&dir,&s,&mut network).await});
  let partial=base.join(format!("{}.part",digest(b"guid")));
  tokio::time::timeout(Duration::from_secs(3),async{loop{if fs::metadata(&partial).map(|m|m.len()as usize==half).unwrap_or(false){break;}tokio::time::sleep(Duration::from_millis(10)).await;}}).await.unwrap();
  state.paused.store(true,Ordering::SeqCst);tokio::time::sleep(Duration::from_millis(250)).await;assert!(!task.is_finished());assert_eq!(fs::metadata(&partial).unwrap().len(),half as u64);
  state.cancel.store(true,Ordering::SeqCst);assert!(tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap().is_err());
  let restarted=VaultState::default();let mut network=0;let result=chunk(&reqwest::Client::new(),&url,"guid",&base,&restarted,&mut network).await.unwrap();assert_eq!(result,data);assert_eq!(network,(raw.len()-half)as u64);assert_eq!(calls.load(Ordering::SeqCst),2);
  let reused=chunk(&reqwest::Client::new(),&url,"guid",&base,&restarted,&mut network).await.unwrap();assert_eq!(reused,data);assert_eq!(calls.load(Ordering::SeqCst),2);worker.join().unwrap();fs::remove_dir_all(base).unwrap();
 }
 #[test]fn paused_progress_has_zero_speed_and_disables_pause_during_finalization(){let state=VaultState::default();state.paused.store(true,Ordering::SeqCst);state.pause_available.store(true,Ordering::SeqCst);emit(&state,Progress{phase:"Downloading".into(),done:40,total:100,files_done:1,files_total:3,network_bytes:70,bytes_per_second:90,paused:false,can_pause:false});let p=state.latest.lock().unwrap().clone().unwrap();assert!(p.paused&&p.can_pause);assert_eq!(p.done,40);assert_eq!(p.bytes_per_second,0);state.paused.store(false,Ordering::SeqCst);state.pause_available.store(false,Ordering::SeqCst);emit(&state,p);let p=state.latest.lock().unwrap().clone().unwrap();assert!(!p.paused&&!p.can_pause);}
 #[test]fn saved_task_validation_rejects_foreign_paths_reserved_metadata_and_bad_offsets(){
  let root=crate::storage::vault();let mut plan=fixture(&root);assert!(validate(&plan).is_ok());
  plan.files.values_mut().next().unwrap().file_chunk_parts[0].offset=u128::MAX;assert!(validate(&plan).is_err());
  let mut plan=fixture(&root);plan.info.path=root.join("other").to_string_lossy().into_owned();assert!(validate(&plan).is_err());
  let mut plan=fixture(&root);let mut f=plan.files.pop_first().unwrap().1;f.filename="package.um.json".into();plan.info.hashes=BTreeMap::from([(f.filename.clone(),f.file_hash.clone())]);plan.files.insert(f.filename.clone(),f);assert!(validate(&plan).is_err());
 }
 #[tokio::test]async fn server_without_range_restarts_only_the_incomplete_block(){
  use std::net::TcpListener;let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());fs::create_dir_all(&base).unwrap();let data=b"complete block".to_vec();let raw=encoded(&data);fs::write(base.join(format!("{}.part",digest(b"guid"))),&raw[..20]).unwrap();
  let server=TcpListener::bind("127.0.0.1:0").unwrap();let address=server.local_addr().unwrap();let body=raw.clone();let worker=std::thread::spawn(move||{let(mut stream,_)=server.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();let mut h=vec![];let mut byte=[0];while !h.ends_with(b"\r\n\r\n"){stream.read_exact(&mut byte).unwrap();h.push(byte[0]);}assert!(String::from_utf8(h).unwrap().to_lowercase().contains("range: bytes=20-"));write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();stream.write_all(&body).unwrap();});
  let mut network=0;let result=chunk(&reqwest::Client::new(),&reqwest::Url::parse(&format!("http://{address}/chunk")).unwrap(),"guid",&base,&VaultState::default(),&mut network).await.unwrap();assert_eq!(result,data);assert_eq!(network,raw.len()as u64);worker.join().unwrap();fs::remove_dir_all(base).unwrap();
 }
 #[test]fn successful_download_cleanup_removes_owned_temporaries_but_preserves_unknown_files(){
  let base=std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());let id=digest(base.to_string_lossy().as_bytes());let stage=base.join(&id);fs::create_dir_all(stage.join("chunks")).unwrap();fs::write(stage.join("chunks").join(format!("{}.chunk",digest(b"block"))),b"cached").unwrap();let temp=stage.join(format!("{}.tmp",uuid::Uuid::new_v4().simple()));fs::write(&temp,b"old assembly").unwrap();fs::write(stage.join("keep.txt"),b"keep").unwrap();assert!(cleanup(&stage,&id).is_err());assert!(!temp.exists());assert_eq!(fs::read(stage.join("keep.txt")).unwrap(),b"keep");fs::remove_file(stage.join("keep.txt")).unwrap();cleanup(&stage,&id).unwrap();assert!(!stage.exists());
 }
}

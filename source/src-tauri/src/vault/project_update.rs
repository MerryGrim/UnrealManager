use super::*;
use tauri::ipc::Channel;

#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct UpdatePreview {
    target: String, token: String, compatible: bool, changed: Vec<String>, added: Vec<String>,
    conflicts: Vec<String>, local_changes: Vec<String>, removed: Vec<String>, unchanged: usize, already_current: usize,
    all_files: usize, backup_name: String, backup_directory: String,
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct UpdateResult { copied: usize, preserved: usize, backup_path: Option<String> }
#[derive(Clone)]
struct Entry { relative: PathBuf, source: PathBuf, hash: String, before: Option<String>, changed: bool, conflict: bool }
struct UpdatePlan { root: PathBuf, backup_root: PathBuf, entries: Vec<Entry>, preview: UpdatePreview }

fn mapped(info: &Package, target: &str) -> Result<(PathBuf,BTreeMap<String,PathBuf>,bool),String> {
    if info.kind != "Project" { return install_files(info,target,None); }
    let descriptor=Path::new(target); no_links(descriptor)?;
    if !descriptor.is_file() || !descriptor.extension().is_some_and(|s|s.eq_ignore_ascii_case("uproject")) {return Err("Выберите существующий файл .uproject".into());}
    let value:serde_json::Value=serde_json::from_slice(&fs::read(descriptor).map_err(io_error)?).map_err(|_|"Не удалось прочитать проект")?;
    let root=descriptor.parent().ok_or("Нет каталога проекта")?;
    let name=descriptor.file_stem().and_then(|n|n.to_str()).ok_or("Некорректное имя проекта")?;
    let (root,mut entries,_)=install_files_mode(info,root.to_str().ok_or("Некорректный путь")?,Some(name),true)?;
    // The destination descriptor carries the user's name, engine and plugin settings.
    entries.retain(|_,p|!p.extension().is_some_and(|s|s.eq_ignore_ascii_case("uproject")));
    let compatible=info.engines.iter().any(|v|v.trim_start_matches("UE_")==value["EngineAssociation"].as_str().unwrap_or(""));
    Ok((root,entries,compatible))
}
fn key(path:&Path)->String {path.to_string_lossy().replace('\\',"/").to_lowercase()}
fn present_hash(path:&Path)->Result<Option<String>,String> {
    present_hash_progress(path, |_|{})
}
fn present_hash_progress(path:&Path,progress:impl FnMut(u64))->Result<Option<String>,String>{
    no_links(path)?;
    if !path.exists(){return Ok(None);}
    if !path.is_file(){return Err(format!("Вместо файла находится каталог: {}",path.display()));}
    Ok(Some(hash_file_progress(path,progress)?))
}
fn make_update(old:&Package,new:&Package,target:&str)->Result<UpdatePlan,String> {
    make_update_progress(old,new,target,&mut |_|{})
}
fn make_update_progress(old:&Package,new:&Package,target:&str,work:&mut dyn FnMut(Progress))->Result<UpdatePlan,String>{
    if old.asset_id.is_empty()||old.asset_id!=new.asset_id||old.kind!=new.kind||old.id==new.id {return Err("Выберите две разные копии одного материала в Vault".into());}
    if !old.engines.iter().any(|e|new.engines.contains(e)){return Err("Для обновления нужны копии для одной версии UE".into());}
    let (root,old_map,_)=mapped(old,target)?;
    let (new_root,new_map,compatible)=mapped(new,target)?;
    if root!=new_root||!root.is_dir(){return Err("Каталог назначения изменился".into());}
    no_links(&root)?;
    let canonical_root=root.canonicalize().map_err(io_error)?;
    for source in [&old.path,&new.path]{
        let source=Path::new(source).canonicalize().map_err(io_error)?;
        if canonical_root.starts_with(&source)||source.starts_with(&canonical_root){return Err("Назначение не должно пересекаться с папками Vault".into());}
    }
    if let Ok(vault)=crate::storage::vault().as_path().canonicalize(){if canonical_root.starts_with(&vault)||vault.starts_with(&canonical_root){return Err("Нельзя обновлять проект внутри Vault или его родительского каталога".into());}}
    // A retained transaction may contain originals needed after an interrupted update.
    for entry in fs::read_dir(&root).map_err(io_error)? {
        if entry.map_err(io_error)?.file_name().to_string_lossy().starts_with(".um-update-"){return Err("Найдена незавершённая операция .um-update-. Сначала восстановите её файлы по journal.json".into());}
    }
    let old_hashes:BTreeMap<_,_>=old_map.iter().map(|(n,p)|(key(p),old.hashes[n].to_lowercase())).collect();
    let new_keys:BTreeSet<_>=new_map.values().map(|p|key(p)).collect();
    let mut backup_root=root.clone();
    if new.kind=="Plugin" {
        let plugin_dir=|map:&BTreeMap<String,PathBuf>|map.values().find(|p|p.extension().is_some_and(|s|s.eq_ignore_ascii_case("uplugin"))).and_then(|p|p.parent()).map(Path::to_path_buf);
        let old_dir=plugin_dir(&old_map).ok_or("Нет описания плагина")?;
        let new_dir=plugin_dir(&new_map).ok_or("Нет описания плагина")?;
        if key(&old_dir)!=key(&new_dir){return Err("Имя плагина изменилось. Требуется отдельная установка".into());}
        backup_root=root.join(new_dir);
        if !backup_root.is_dir(){return Err("Плагин ещё не установлен в выбранное место".into());}
    }
    let mut preview=UpdatePreview{target:backup_root.to_string_lossy().into(),token:String::new(),compatible,changed:vec![],added:vec![],conflicts:vec![],local_changes:vec![],removed:vec![],unchanged:0,already_current:0,all_files:new_map.len(),backup_directory:crate::storage::vault().join("Backups").display().to_string(),backup_name:format!("{}_backup_{}",backup_root.file_name().unwrap_or_default().to_string_lossy(),chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"))};
    let mut entries=Vec::new(); let mut snapshot=Vec::new();
    work(local_progress("Проверяем файлы проекта",0,0,0,new_map.len()));
    let mut bytes=0u64;for path in new_map.values(){let p=root.join(path);no_links(&p)?;if p.is_file(){bytes=bytes.saturating_add(fs::metadata(p).map_err(io_error)?.len());}}
    let mut progress=FileProgress::new("Проверяем файлы проекта",bytes,new_map.len(),work);
    for (name,path) in new_map {
        let hash=new.hashes[&name].to_lowercase();
        let before=present_hash_progress(&root.join(&path),|n|progress.advance(n))?;
        let baseline=old_hashes.get(&key(&path));
        let changed=baseline!=Some(&hash);
        let conflict=changed && before.as_ref()!=Some(&hash) && before.as_ref()!=baseline;
        let display=path.to_string_lossy().into_owned();
        if before.as_ref()!=baseline && before.as_ref()!=Some(&hash){preview.local_changes.push(display.clone());}
        if !changed{preview.unchanged+=1;}
        else if before.as_ref()==Some(&hash){preview.already_current+=1;}
        else if conflict{preview.conflicts.push(display);}
        else if baseline.is_none(){preview.added.push(display);}
        else{preview.changed.push(display);}
        snapshot.push((key(&path),hash.clone(),before.clone()));
        entries.push(Entry{relative:path,source:Path::new(&new.path).join(relative(&name)?),hash,before,changed,conflict});
        progress.file_done();
    }
    for path in old_map.values().filter(|p|!new_keys.contains(&key(p))){preview.removed.push(path.to_string_lossy().into_owned());}
    preview.token=digest(&serde_json::to_vec(&(old.id.clone(),new.id.clone(),root.to_string_lossy(),old_hashes,snapshot)).map_err(|_|"Не удалось проверить файлы")?);
    Ok(UpdatePlan{root,backup_root,entries,preview})
}

fn copy_new_progress(source:&Path,target:&Path,mut progress:impl FnMut(u64))->Result<(),String>{
    no_links(source)?; no_links(target)?;
    let mut input=File::open(source).map_err(io_error)?;
    let mut output=OpenOptions::new().write(true).create_new(true).open(target).map_err(io_error)?;
    let mut buffer=[0u8;65536];loop{let n=input.read(&mut buffer).map_err(io_error)?;if n==0{break;}output.write_all(&buffer[..n]).map_err(io_error)?;progress(n as u64);}output.sync_all().map_err(io_error)?;
    Ok(())
}
fn backup_tree(source:&Path,destination:&Path,notify:&impl Fn(String),work:&mut dyn FnMut(Progress))->Result<(),String>{
    // Never follow junctions or merge with an existing backup.
    no_links(source)?;no_links(destination)?;
    work(local_progress("Создаём резервную копию…",0,0,0,0));
    let mut files=0;let mut bytes=0u64;let mut last=Instant::now();
    for item in walkdir::WalkDir::new(source).follow_links(false){let item=item.map_err(|_|"Не удалось прочитать папку резервной копии")?;no_links(item.path())?;if item.file_type().is_file(){files+=1;bytes=bytes.saturating_add(item.metadata().map_err(|_|"Не удалось прочитать папку резервной копии")?.len());}if last.elapsed()>=Duration::from_millis(250){work(local_progress("Создаём резервную копию…",0,0,files,0));last=Instant::now();}}
    let mut progress=FileProgress::new("Создаём резервную копию…",bytes.saturating_mul(3),files,work);
    fs::create_dir(destination).map_err(io_error)?;
    for item in walkdir::WalkDir::new(source).follow_links(false){
        let item=item.map_err(|_|"Не удалось прочитать папку резервной копии")?;
        no_links(item.path())?;
        let rel=item.path().strip_prefix(source).map_err(|_|"Некорректный путь копии")?;
        if rel.as_os_str().is_empty(){continue;}
        let out=destination.join(rel);
        if item.file_type().is_dir(){fs::create_dir(&out).map_err(io_error)?;}
        else if item.file_type().is_file(){notify(format!("Резервная копия: {}",rel.display()));copy_new_progress(item.path(),&out,|n|progress.advance(n))?;if hash_file_progress(item.path(),|n|progress.advance(n))?!=hash_file_progress(&out,|n|progress.advance(n))?{return Err("Файлы меняются во время резервного копирования. Закройте Unreal Editor".into());}progress.file_done();}
        else{return Err("Особые файлы в резервной копии не поддерживаются".into());}
    }
    Ok(())
}
fn apply_update(plan:UpdatePlan,mode:&str,backup:bool,backup_base:&Path,notify:impl Fn(String))->Result<UpdateResult,String>{
    apply_update_progress(plan,mode,backup,backup_base,notify,&mut |_|{})
}
fn apply_update_progress(plan:UpdatePlan,mode:&str,backup:bool,backup_base:&Path,notify:impl Fn(String),work:&mut dyn FnMut(Progress))->Result<UpdateResult,String>{
    if !["changed","all"].contains(&mode){return Err("Неизвестный режим обновления".into());}
    let chosen:Vec<_>=plan.entries.iter().filter(|e|mode=="all"||(e.changed&&!e.conflict&&e.before.as_ref()!=Some(&e.hash))).collect();
    let mut result=UpdateResult{copied:0,preserved:if mode=="changed"{plan.preview.conflicts.len()}else{0},backup_path:None};
    if chosen.is_empty(){return Ok(result);}
    let mut source_bytes=0u64;for e in &chosen{no_links(&e.source)?;source_bytes=source_bytes.saturating_add(fs::metadata(&e.source).map_err(io_error)?.len());}
    let mut progress=FileProgress::new("Проверяем файлы Vault",source_bytes,chosen.len(),work);
    for e in &chosen{if hash_file_progress(&e.source,|n|progress.advance(n))?!=e.hash{return Err("Файл Vault повреждён. Обновление отменено".into());}progress.file_done();}drop(progress);
    if backup{
        no_links(backup_base)?;fs::create_dir_all(backup_base).map_err(io_error)?;
        let destination=backup_base.join(format!("{}_{}",plan.preview.backup_name,&uuid::Uuid::new_v4().to_string()[..8]));
        // On failure retain the partial backup and report its location; originals are untouched.
        backup_tree(&plan.backup_root,&destination,&notify,work).map_err(|e|format!("{e}. Неполная копия: {}. Исходные файлы не изменены",destination.display()))?;
        result.backup_path=Some(destination.to_string_lossy().into());
    }
    let mut target_bytes=0u64;for e in &chosen{let p=plan.root.join(&e.relative);no_links(&p)?;if p.is_file(){target_bytes=target_bytes.saturating_add(fs::metadata(p).map_err(io_error)?.len());}}
    let mut progress=FileProgress::new("Проверяем файлы проекта",target_bytes,chosen.len(),work);
    for e in &chosen{if present_hash_progress(&plan.root.join(&e.relative),|n|progress.advance(n))?!=e.before{return Err("Файлы изменились после проверки. Выполните проверку заново".into());}progress.file_done();}drop(progress);
    let transaction=plan.root.join(format!(".um-update-{}",uuid::Uuid::new_v4()));
    fs::create_dir(&transaction).map_err(io_error)?;
    // Durable mapping permits manual recovery after power loss. Originals remain on the same volume.
    let journal:Vec<_>=chosen.iter().enumerate().map(|(i,e)|serde_json::json!({"path":e.relative,"original":format!("{i}.old"),"staged":format!("{i}.new"),"before":e.before,"after":e.hash})).collect();
    let mut log=OpenOptions::new().write(true).create_new(true).open(transaction.join("journal.json")).map_err(io_error)?;
    log.write_all(&serde_json::to_vec_pretty(&journal).map_err(|_|"Не удалось создать журнал")?).map_err(io_error)?;log.sync_all().map_err(io_error)?;drop(log);
    let mut moved:Vec<usize>=vec![]; let mut installed:Vec<usize>=vec![];
    let operation=(||->Result<(),String>{
        let mut progress=FileProgress::new("Подготавливаем замену…",source_bytes.saturating_mul(2),chosen.len(),work);
        for (i,e) in chosen.iter().enumerate(){
            notify(format!("Подготовка: {} / {}",i+1,chosen.len()));
            let staged=transaction.join(format!("{i}.new"));copy_new_progress(&e.source,&staged,|n|progress.advance(n))?;
            if hash_file_progress(&staged,|n|progress.advance(n))?!=e.hash{return Err("Проверка новой копии не пройдена".into());}progress.file_done();
        }
        drop(progress);let mut progress=FileProgress::new("Обновляем файлы проекта",target_bytes,chosen.len(),work);
        for (i,e) in chosen.iter().enumerate(){
            let target=plan.root.join(&e.relative);no_links(&target)?;
            if present_hash_progress(&target,|n|progress.advance(n))?!=e.before{return Err("Файлы проекта изменились во время обновления".into());}
            fs::create_dir_all(target.parent().ok_or("Некорректный путь")?).map_err(io_error)?;
            if e.before.is_some(){fs::rename(&target,transaction.join(format!("{i}.old"))).map_err(io_error)?;moved.push(i);}
            fs::rename(transaction.join(format!("{i}.new")),&target).map_err(io_error)?;installed.push(i);
            notify(format!("Обновление: {} / {}",i+1,chosen.len()));
            progress.file_done();
        }
        Ok(())
    })();
    if let Err(error)=operation{
        work(local_progress("Восстанавливаем исходные файлы",0,0,0,moved.len()+installed.len()));
        let mut failed=false;
        for &i in installed.iter().rev(){let e=chosen[i];let path=plan.root.join(&e.relative);if present_hash(&path).ok().flatten()==Some(e.hash.clone()){if fs::remove_file(path).is_err(){failed=true;}}else{failed=true;}}
        for &i in moved.iter().rev(){let target=plan.root.join(&chosen[i].relative);if no_links(&target).is_err()||target.exists()||fs::rename(transaction.join(format!("{i}.old")),target).is_err(){failed=true;}}
        if failed{return Err(format!("{error}. Восстановление требует внимания. Оригиналы и журнал: {}",transaction.display()));}
        cleanup(&transaction,chosen.len()).map_err(|e|format!("{error}. {e}"))?;
        return Err(format!("{error}. Изменения отменены. Резервная копия: {}",result.backup_path.as_deref().unwrap_or("не запрашивалась")));
    }
    result.copied=chosen.len();
    work(local_progress("Очищаем временные файлы",0,0,0,chosen.len()));
    cleanup(&transaction,chosen.len()).map_err(|e|format!("Обновление выполнено, но {e}"))?;
    work(local_progress("Файлы установлены и проверены",1,1,chosen.len(),chosen.len()));
    Ok(result)
}
fn cleanup(transaction:&Path,count:usize)->Result<(),String>{
    no_links(transaction)?;
    for i in 0..count{for ext in ["old","new"]{let path=transaction.join(format!("{i}.{ext}"));if path.exists(){fs::remove_file(path).map_err(io_error)?;}}}
    fs::remove_file(transaction.join("journal.json")).map_err(io_error)?;
    fs::remove_dir(transaction).map_err(|_|format!("не удалось убрать временный каталог {}",transaction.display()))
}
#[tauri::command]
pub async fn project_update_preview(webview:tauri::Webview,old_id:String,new_id:String,target:String,on_work_progress:Option<tauri::ipc::JavaScriptChannelId>)->Result<UpdatePreview,String>{
    let on_work_progress:Option<Channel<Progress>>=on_work_progress.map(|id|id.channel_on(webview));
    tauri::async_runtime::spawn_blocking(move||Ok(make_update_progress(&package(&old_id)?,&package(&new_id)?,&target,&mut |p|{if let Some(channel)=&on_work_progress{let _=channel.send(p);}})?.preview)).await.map_err(|_|"Проверка прервана")?
}
#[tauri::command]
pub async fn project_update_apply(state:tauri::State<'_,VaultState>,webview:tauri::Webview,old_id:String,new_id:String,target:String,token:String,mode:String,backup:bool,accept_version:bool,on_progress:Channel<String>,on_work_progress:Option<tauri::ipc::JavaScriptChannelId>)->Result<UpdateResult,String>{
    let on_work_progress:Option<Channel<Progress>>=on_work_progress.map(|id|id.channel_on(webview));
    let _update_operation=crate::app_updates::operation()?;
    if state.active.swap(true,Ordering::SeqCst){return Err("Дождитесь текущей операции Vault".into());}let _busy=Busy(&state.active);
    state.updating.store(true,Ordering::SeqCst);let _updating=Busy(&state.updating);
    tauri::async_runtime::spawn_blocking(move||{
        let mut work=|p|{if let Some(channel)=&on_work_progress{let _=channel.send(p);}};
        let plan=make_update_progress(&package(&old_id)?,&package(&new_id)?,&target,&mut work)?;
        if plan.preview.token!=token{return Err("Состав файлов изменился. Выполните проверку заново".into());}
        if !plan.preview.compatible&&!accept_version{return Err("Подтвердите несовпадение версии UE".into());}
        apply_update_progress(plan,&mode,backup,&crate::storage::vault().as_path().join("Backups"),|s|{let _=on_progress.send(s);},&mut work)
    }).await.map_err(|_|"Обновление прервано")?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture{root:PathBuf,old:Package,new:Package,target:String}
    impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
    fn put(root:&Path,name:&str,data:&[u8]){let p=root.join(name);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,data).unwrap();}
    fn package_at(root:&Path,id:&str,files:&[(&str,&[u8])])->Package{
        for (n,b) in files{put(root,n,b);}
        Package{artifact_id:"v".into(),selected_engine:"5.8".into(),build_version:id.into(),source:String::new(),asset_id:"asset".into(),id:id.into(),title:"Demo".into(),kind:"Asset".into(),engines:vec!["UE_5.8".into()],bytes:0,download_bytes:0,files:files.len(),path:root.to_string_lossy().into(),hashes:files.iter().map(|(n,b)|(n.to_string(),digest(b))).collect()}
    }
    fn fixture()->Fixture{
        let root=std::env::temp_dir().join(format!("um-update-test-{}",uuid::Uuid::new_v4()));
        let files:Vec<(&str,&[u8])>=vec![("Content/A.uasset",b"old-a"),("Content/B.uasset",b"same-b"),("Content/C.uasset",b"old-c"),("Content/Removed.uasset",b"removed"),("Content/Deleted.uasset",b"old-d")];
        let old=package_at(&root.join("old"),"old",&files);
        let new=package_at(&root.join("new"),"new",&[("Content/A.uasset",b"new-a"),("Content/B.uasset",b"same-b"),("Content/C.uasset",b"new-c"),("Content/New.uasset",b"new"),("Content/Deleted.uasset",b"new-d")]);
        for (n,b) in files{put(&root.join("project"),n,b);}
        put(&root.join("project"),"Demo.uproject",br#"{"EngineAssociation":"5.8"}"#);
        put(&root.join("project"),"Content/C.uasset",b"my-c");
        put(&root.join("project"),"Content/B.uasset",b"my-b");
        fs::remove_file(root.join("project/Content/Deleted.uasset")).unwrap();
        let target=root.join("project/Demo.uproject").to_string_lossy().into();Fixture{root,old,new,target}
    }
    #[test]
    fn delta_only_copies_author_changes_and_preserves_local_edits(){
        let f=fixture();let plan=make_update(&f.old,&f.new,&f.target).unwrap();
        assert_eq!(plan.preview.changed.len(),1);assert_eq!(plan.preview.added.len(),1);assert_eq!(plan.preview.conflicts.len(),2);assert_eq!(plan.preview.local_changes.len(),3);assert_eq!(plan.preview.unchanged,1);assert_eq!(plan.preview.removed.len(),1);
        let b_time=fs::metadata(f.root.join("project/Content/B.uasset")).unwrap().modified().unwrap();
        let mut progress=Vec::new();let result=apply_update_progress(plan,"changed",true,&f.root.join("backups"),|_|{},&mut |p|progress.push(p)).unwrap();assert_eq!(result.copied,2);assert_eq!(result.preserved,2);
        assert!(progress.iter().all(|p|p.network_bytes==0&&!p.can_pause));assert_eq!(progress.last().unwrap().phase,"Файлы установлены и проверены");
        let staged=progress.iter().filter(|p|p.phase=="Подготавливаем замену…").last().unwrap();assert_eq!(staged.files_done,2);assert_eq!(staged.done,2*(b"new-a".len()+b"new".len())as u64);assert_eq!(staged.total,staged.done);
        let backed_up=progress.iter().filter(|p|p.phase=="Создаём резервную копию…").last().unwrap();assert!(backed_up.total>0);assert_eq!(backed_up.done,backed_up.total);assert_eq!(backed_up.files_done,backed_up.files_total);
        assert_eq!(fs::read(f.root.join("project/Content/A.uasset")).unwrap(),b"new-a");
        assert_eq!(fs::read(f.root.join("project/Content/C.uasset")).unwrap(),b"my-c");
        assert_eq!(fs::read(f.root.join("project/Content/B.uasset")).unwrap(),b"my-b");
        assert_eq!(fs::metadata(f.root.join("project/Content/B.uasset")).unwrap().modified().unwrap(),b_time);
        assert!(!f.root.join("project/Content/Deleted.uasset").exists());assert!(f.root.join("project/Content/Removed.uasset").exists());
        let backup=PathBuf::from(result.backup_path.unwrap());assert!(backup.file_name().unwrap().to_string_lossy().starts_with("project_backup_"));
        assert_eq!(fs::read(backup.join("Content/A.uasset")).unwrap(),b"old-a");assert_eq!(fs::read(backup.join("Content/C.uasset")).unwrap(),b"my-c");assert!(backup.join("Demo.uproject").exists());
        let repeat=make_update(&f.old,&f.new,&f.target).unwrap();assert_eq!(repeat.preview.already_current,2);assert!(repeat.preview.changed.is_empty());
    }
    #[test]
    fn replace_all_restores_all_package_files_and_keeps_unrelated_files(){
        let f=fixture();let result=apply_update(make_update(&f.old,&f.new,&f.target).unwrap(),"all",false,&f.root.join("backups"),|_|{}).unwrap();
        assert_eq!(result.copied,5);assert!(result.backup_path.is_none());
        for (n,h) in &f.new.hashes{assert_eq!(&hash_file(&f.root.join("project").join(n)).unwrap(),h);}
        assert!(f.root.join("project/Content/Removed.uasset").exists());assert!(f.root.join("project/Demo.uproject").exists());
    }
    #[test]
    fn backup_failure_and_corrupt_source_do_not_modify_project(){
        let f=fixture();put(&f.root,"backup-file",b"not a directory");
        assert!(apply_update(make_update(&f.old,&f.new,&f.target).unwrap(),"all",true,&f.root.join("backup-file"),|_|{}).is_err());
        put(&f.root.join("new"),"Content/A.uasset",b"corrupt");
        assert!(apply_update(make_update(&f.old,&f.new,&f.target).unwrap(),"all",false,&f.root.join("backups"),|_|{}).is_err());
        assert_eq!(fs::read(f.root.join("project/Content/A.uasset")).unwrap(),b"old-a");
    }
    #[test]
    fn concurrent_change_rolls_back_completed_replacements(){
        let f=fixture();let plan=make_update(&f.old,&f.new,&f.target).unwrap();let first=plan.entries[0].relative.clone();let second=plan.entries[1].relative.clone();let original=fs::read(f.root.join("project").join(&first)).unwrap();
        let mut progress=Vec::new();let error=apply_update_progress(plan,"all",false,&f.root.join("backups"),|s|{if s.starts_with("Обновление: 1 /"){put(&f.root.join("project"),second.to_str().unwrap(),b"external-edit");}},&mut |p|progress.push(p)).err().unwrap();
        assert!(!progress.iter().any(|p|p.phase=="Файлы установлены и проверены"));assert!(progress.iter().any(|p|p.phase=="Восстанавливаем исходные файлы"));
        assert!(error.contains("Изменения отменены"));assert_eq!(fs::read(f.root.join("project").join(first)).unwrap(),original);assert_eq!(fs::read(f.root.join("project").join(second)).unwrap(),b"external-edit");
        assert!(make_update(&f.old,&f.new,&f.target).is_ok());
    }
    #[test]
    fn stale_preview_changes_token_and_mismatched_material_is_rejected(){
        let mut f=fixture();let before=make_update(&f.old,&f.new,&f.target).unwrap().preview.token;put(&f.root.join("project"),"Content/A.uasset",b"edit");assert_ne!(before,make_update(&f.old,&f.new,&f.target).unwrap().preview.token);
        f.new.asset_id="different".into();assert!(make_update(&f.old,&f.new,&f.target).is_err());
    }
    #[test]
    fn complete_project_preserves_renamed_descriptor_and_engine(){
        let mut f=fixture();f.old.kind="Project".into();f.new.kind="Project".into();
        for p in [&mut f.old,&mut f.new]{put(Path::new(&p.path),"Author.uproject",br#"{"EngineAssociation":"5.3"}"#);p.hashes.insert("Author.uproject".into(),digest(br#"{"EngineAssociation":"5.3"}"#));}
        let original=fs::read(&f.target).unwrap();apply_update(make_update(&f.old,&f.new,&f.target).unwrap(),"all",false,&f.root.join("backups"),|_|{}).unwrap();assert_eq!(fs::read(&f.target).unwrap(),original);assert!(!f.root.join("project/Author.uproject").exists());
    }
    #[test]
    fn plugin_update_stays_in_plugin_folder(){
        let mut f=fixture();
        f.old=package_at(&f.root.join("plugin-old"),"old",&[("Demo/Demo.uplugin",b"old"),("Demo/Content/Thing.uasset",b"same")]);f.old.kind="Plugin".into();
        f.new=package_at(&f.root.join("plugin-new"),"new",&[("Demo/Demo.uplugin",b"new"),("Demo/Content/Thing.uasset",b"same")]);f.new.kind="Plugin".into();
        put(&f.root.join("project"),"Plugins/Demo/Demo.uplugin",b"old");put(&f.root.join("project"),"Plugins/Demo/Content/Thing.uasset",b"same");
        let result=apply_update(make_update(&f.old,&f.new,&f.target).unwrap(),"changed",true,&f.root.join("backups"),|_|{}).unwrap();assert_eq!(result.copied,1);let backup=PathBuf::from(result.backup_path.unwrap());assert!(backup.join("Demo.uplugin").exists());assert!(!backup.join("Demo.uproject").exists());assert!(f.root.join("project/Content/A.uasset").exists());
    }
    #[test]
    fn backup_names_never_replace_previous_copy(){
        let f=fixture();let make=||{let mut p=make_update(&f.old,&f.new,&f.target).unwrap();p.preview.backup_name="project_backup_2026-09-26_12-00-00".into();p};
        let first=apply_update(make(),"all",true,&f.root.join("backups"),|_|{}).unwrap().backup_path.unwrap();
        let second=apply_update(make(),"all",true,&f.root.join("backups"),|_|{}).unwrap().backup_path.unwrap();
        assert_ne!(first,second);assert_eq!(fs::read(Path::new(&first).join("Content/A.uasset")).unwrap(),b"old-a");assert_eq!(fs::read(Path::new(&second).join("Content/A.uasset")).unwrap(),b"new-a");
    }
}

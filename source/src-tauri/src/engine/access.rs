//! Read-only Windows permission probes. They never create a probe file, alter an
//! ACL/attribute, or request elevation themselves. Runtime checks remain in place.
use super::*;

const DELETE:u32=0x0001_0000;
const ADD_FILE:u32=0x2;
const ADD_DIRECTORY:u32=0x4;
const DELETE_CHILD:u32=0x40;

pub(super) fn contextual(operation:&str,path:&Path,error:std::io::Error)->String{
    format!("Ошибка файловой системы ({operation}): {}: {error}",path.display())
}
fn denied(operation:&str,path:&Path,error:std::io::Error)->String{
    if error.raw_os_error()==Some(5){crate::elevation::required(operation,path)}
    else{contextual(operation,path,error)}
}
#[cfg(windows)]
fn probe(path:&Path,rights:u32)->std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    // BACKUP_SEMANTICS permits a directory handle; OPEN_REPARSE_POINT avoids
    // following the leaf if it changed since no_links. The handle is read-only.
    fs::OpenOptions::new().access_mode(rights).share_mode(7)
        .custom_flags(0x0200_0000|0x0020_0000).open(path).map(drop)
}
#[cfg(not(windows))]
fn probe(_path:&Path,_rights:u32)->std::io::Result<()>{Ok(())}
fn nearest_directory(path:&Path)->Result<PathBuf,String>{
    let mut current=path;
    loop{no_links(current)?;match fs::metadata(current){
        Ok(meta)=>{if !meta.is_dir(){return Err(format!("Путь не является папкой: {}",current.display()));}return Ok(current.into());},
        Err(error)if error.kind()==std::io::ErrorKind::NotFound=>current=current.parent().ok_or("Не найдена родительская папка")?,
        Err(error)=>return Err(contextual("Проверка папки",current,error)),
    }}
}
pub(super) fn create_file(path:&Path,operation:&str)->Result<(),String>{
    no_links(path)?;
    let parent=path.parent().ok_or("Не найдена родительская папка")?;
    let existing=nearest_directory(parent)?;
    let rights=if existing==parent{ADD_FILE}else{ADD_DIRECTORY};
    probe(&existing,rights).map_err(|e|denied(operation,&existing,e))
}
pub(super) fn create_directory(path:&Path,operation:&str)->Result<(),String>{
    no_links(path)?;
    let existing=nearest_directory(path)?;
    let rights=if existing==path{ADD_FILE|ADD_DIRECTORY}else{ADD_DIRECTORY};
    probe(&existing,rights).map_err(|e|denied(operation,&existing,e))
}
pub(super) fn remove(path:&Path,operation:&str)->Result<(),String>{
    no_links(path)?;
    remove_entry(path,operation)
}
pub(super) fn remove_entry(path:&Path,operation:&str)->Result<(),String>{
    // Deletion inventories may include a junction itself. Never follow it or
    // reject supported no-follow deletion merely because its leaf is a link.
    no_links(path.parent().ok_or("Не найдена родительская папка")?)?;
    let meta=match fs::symlink_metadata(path){Ok(meta)=>meta,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(()),Err(e)=>return Err(contextual("Проверка файла",path,e))};
    // Read-only is an attribute, not proof that elevation will solve the issue.
    if meta.is_file()&&meta.permissions().readonly(){return Err(format!("Файл доступен только для чтения: {}. Снимите этот атрибут и повторите операцию.",path.display()));}
    match probe(path,DELETE){
        Ok(())=>Ok(()),
        Err(file_error)if file_error.raw_os_error()==Some(5)=>{
            let parent=path.parent().ok_or("Не найдена родительская папка")?;
            no_links(parent)?;
            // Windows accepts DELETE on the file OR DELETE_CHILD on its parent.
            // Do not demand both: normal engine roots may deny DELETE_CHILD.
            probe(parent,DELETE_CHILD).map_err(|parent_error|{
                if parent_error.raw_os_error()==Some(5){denied(operation,path,file_error)}
                else{contextual("Проверка удаления из папки",parent,parent_error)}
            })
        },
        Err(error)=>Err(contextual("Проверка удаления файла",path,error)),
    }
}
pub(super) fn replace_file(path:&Path,operation:&str)->Result<(),String>{
    // Atomic replacement also needs permission to create the new sibling.
    remove(path,operation)?;create_file(path,operation)
}
pub(super) fn plan(plan:&Plan,stage:&Path,mut progress:impl FnMut(u64,u64,Option<&str>))->Result<(),String>{
    // Mandatory registration is checked (or broker-approved) first by the caller.
    let root=target(&plan.info.path)?;
    create_directory(stage,"staging")?;
    replace_file(&stage.join("journal.json"),"staging")?;
    create_file(&root.join(".um-engine-update.json"),"engineFiles")?;
    if super::resume::commit_complete(stage)?{return Ok(());}
    let mut checks:BTreeMap<&str,(bool,bool)>=BTreeMap::new();
    for(name,old)in &plan.old{
        let changed=match plan.files.get(name){Some(new)=>!old.file_hash.eq_ignore_ascii_case(&new.file_hash),None=>plan.remove_unselected||!plan.tags.contains_key(name)};
        if changed{checks.insert(name,(true,false));}
    }
    for(name,new)in &plan.files{
        if plan.old.get(name).is_none_or(|old|!old.file_hash.eq_ignore_ascii_case(&new.file_hash)){
            checks.entry(name).or_default().1=true;
        }
    }
    let total=checks.len()as u64;let mut done=0;let mut last=std::time::Instant::now();progress(0,total,None);
    for(name,(deleting,creating))in checks{
        let path=root.join(relative(name)?);if deleting{remove(&path,"engineFiles")?;}if creating{create_file(&path,"engineFiles")?;}
        done+=1;if last.elapsed()>=std::time::Duration::from_millis(250)||done==total{progress(done,total,Some(name));last=std::time::Instant::now();}
    }
    Ok(())
}
pub(super) fn operations(root:&Path,stage:&Path,ops:&[super::install::Operation],operation:&str)->Result<(),String>{
    operations_reported(root,stage,ops,operation,|_,_,_|{})
}
pub(super) fn operations_reported(root:&Path,stage:&Path,ops:&[super::install::Operation],operation:&str,mut progress:impl FnMut(u64,u64,Option<&str>))->Result<(),String>{
    create_directory(stage,operation)?;
    replace_file(&stage.join("journal.json"),operation)?;
    create_file(&root.join(".um-engine-update.json"),operation)?;
    let total=ops.len()as u64;let mut done=0;let mut last=std::time::Instant::now();progress(0,total,None);
    for op in ops{
        let rel=relative(&op.name)?;let target=root.join(&rel);
        if target.exists(){remove(&target,operation)?;}
        if op.new_hash.is_some(){create_file(&target,operation)?;}
        let original=stage.join("originals").join(&rel);
        if original.exists(){remove(&original,operation)?;}
        create_file(&original,operation)?;
        let source=stage.join("files").join(&rel);
        if source.exists(){remove(&source,operation)?;}
        done+=1;if last.elapsed()>=std::time::Duration::from_millis(250)||done==total{progress(done,total,Some(&op.name));last=std::time::Instant::now();}
    }
    Ok(())
}

#[cfg(test)]
#[path="access_tests.rs"]
mod tests;

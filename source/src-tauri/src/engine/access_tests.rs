use super::*;

struct Fixture{root:PathBuf,parent:PathBuf,id:String,#[cfg(windows)]acls:Vec<(PathBuf,Vec<u8>)>}
impl Fixture{
    fn new()->Self{
        let parent=std::env::var_os("UM_ENGINE_ACCESS_FIXTURE_ROOT").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        no_links(&parent).unwrap();fs::create_dir_all(&parent).unwrap();
        let id=uuid::Uuid::new_v4().to_string();let root=parent.join(format!("um-access-test-{id}"));fs::create_dir(&root).unwrap();
        Self{root,parent,id,#[cfg(windows)]acls:Vec::new()}
    }
    fn file(&self,name:&str)->PathBuf{let path=self.root.join(name);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(&path,b"own fixture").unwrap();path}
    #[cfg(windows)]
    fn dacl(&mut self,path:&Path,sddl:&str){
        self.acls.push((path.into(),security::get(path)));
        security::set_sddl(path,sddl);
    }
}
impl Drop for Fixture{fn drop(&mut self){
    if self.root!=self.parent.join(format!("um-access-test-{}",self.id))||no_links(&self.root).is_err(){return;}
    #[cfg(windows)]for(path,original)in self.acls.iter().rev(){if !path.starts_with(&self.root)||no_links(path).is_err(){return;}security::restore(path,original);}
    fn clear(path:&Path,count:&mut usize)->Result<(),String>{
        *count+=1;if *count>64{return Err("Fixture bound".into());}no_links(path)?;
        let meta=fs::symlink_metadata(path).map_err(io)?;
        if meta.is_dir(){for entry in fs::read_dir(path).map_err(io)?{clear(&entry.map_err(io)?.path(),count)?;}fs::remove_dir(path).map_err(io)}
        else{if meta.permissions().readonly(){let mut p=meta.permissions();p.set_readonly(false);fs::set_permissions(path,p).map_err(io)?;}fs::remove_file(path).map_err(io)}
    }
    let _=clear(&self.root,&mut 0);
}}
fn assert_error_path(error:&str,path:&Path){
    if let Some(json)=error.strip_prefix("UM_ELEVATION_REQUIRED:"){
        let envelope:serde_json::Value=serde_json::from_str(json).unwrap();
        assert_eq!(envelope["operation"],"epicRecord");
        assert_eq!(PathBuf::from(envelope["path"].as_str().expect("exact error path")),path);
    }else{
        assert!(crate::elevation::is_elevated());
        assert!(error.contains(path.to_str().unwrap()));
    }
}

#[test]
fn ordinary_probes_never_modify_or_create_files(){
    let f=Fixture::new();let old=f.file("records/item");let before=fs::read(&old).unwrap();let metadata=fs::metadata(&old).unwrap();
    replace_file(&old,"epicRecord").unwrap();create_directory(&f.root.join("new/deep/stage"),"staging").unwrap();
    assert_eq!(fs::read(&old).unwrap(),before);assert_eq!(fs::metadata(&old).unwrap().modified().unwrap(),metadata.modified().unwrap());
    assert!(!f.root.join("new").exists());assert_eq!(fs::read_dir(old.parent().unwrap()).unwrap().count(),1);
}
#[test]
fn readonly_is_explicit_and_never_misclassified_as_uac(){
    let f=Fixture::new();let path=f.file("readonly");let mut permissions=fs::metadata(&path).unwrap().permissions();permissions.set_readonly(true);fs::set_permissions(&path,permissions).unwrap();
    let error=replace_file(&path,"epicRecord").unwrap_err();assert!(error.contains("только для чтения"));assert!(!error.contains("UM_ELEVATION_REQUIRED:"));assert_eq!(fs::read(&path).unwrap(),b"own fixture");
}
#[test]
fn contextual_errors_retain_operation_path_and_os_code(){
    let path=Path::new("X:/Fixture/engine/file");let text=contextual("rename",path,std::io::Error::from_raw_os_error(5));
    assert!(text.contains("rename")&&text.contains("X:/Fixture/engine/file")&&text.contains('5'));
}
#[test]
fn already_restored_record_does_not_request_a_dead_helper(){
    let f=Fixture::new();let path=f.file("record.item");let before=serde_json::json!({"version":"old"});let after=serde_json::json!({"version":"new"});
    fs::write(&path,serde_json::to_vec(&before).unwrap()).unwrap();let change=Some((path.clone(),before.clone(),after.clone()));
    let mut called=false;
    super::super::install::registration_update_if_needed(&change,true,||{called=true;Err("dead helper".into())}).unwrap();assert!(!called);
    assert!(super::super::install::registration_update_if_needed(&change,false,||{called=true;Err("dead helper".into())}).is_err());assert!(called);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap(),before);
}

#[cfg(windows)]
mod security{
    use super::*;use std::{ffi::c_void,os::windows::ffi::OsStrExt};
    #[link(name="advapi32")]unsafe extern "system"{
        fn GetFileSecurityW(path:*const u16,info:u32,descriptor:*mut c_void,length:u32,needed:*mut u32)->i32;
        fn SetFileSecurityW(path:*const u16,info:u32,descriptor:*const c_void)->i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text:*const u16,revision:u32,descriptor:*mut *mut c_void,size:*mut u32)->i32;
    }
    #[link(name="kernel32")]unsafe extern "system"{fn LocalFree(memory:*mut c_void)->*mut c_void;}
    fn wide(path:&Path)->Vec<u16>{path.as_os_str().encode_wide().chain([0]).collect()}
    pub fn get(path:&Path)->Vec<u8>{unsafe{
        let path=wide(path);let mut needed=0;GetFileSecurityW(path.as_ptr(),4,std::ptr::null_mut(),0,&mut needed);assert!(needed>0);
        let mut bytes=vec![0u8;needed as usize];assert_ne!(GetFileSecurityW(path.as_ptr(),4,bytes.as_mut_ptr().cast(),needed,&mut needed),0);bytes
    }}
    pub fn restore(path:&Path,descriptor:&[u8]){unsafe{let path=wide(path);let _=SetFileSecurityW(path.as_ptr(),4,descriptor.as_ptr().cast());}}
    pub fn set_sddl(path:&Path,sddl:&str){unsafe{
        let text:Vec<u16>=sddl.encode_utf16().chain([0]).collect();let mut descriptor=std::ptr::null_mut();
        assert_ne!(ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(),1,&mut descriptor,std::ptr::null_mut()),0);
        let result=SetFileSecurityW(wide(path).as_ptr(),4|0x8000_0000,descriptor);LocalFree(descriptor);assert_ne!(result,0);
    }}
}

#[cfg(windows)]
#[test]
fn parent_delete_child_is_not_required_when_file_delete_is_allowed(){
    let mut f=Fixture::new();let path=f.file("allowed");let root=f.root.clone();
    // Directory Read/Write/Execute permits ADD_FILE but excludes DELETE_CHILD.
    f.dacl(&root,"D:P(A;;FRFWFX;;;WD)");
    assert_eq!(probe(&root,DELETE_CHILD).unwrap_err().raw_os_error(),Some(5));
    assert!(probe(&path,DELETE).is_ok());replace_file(&path,"engineFiles").unwrap();
    assert_eq!(fs::read(&path).unwrap(),b"own fixture");
}
#[cfg(windows)]
#[test]
fn protected_epic_record_is_caught_before_any_replacement(){
    let mut f=Fixture::new();let path=f.file("item");let root=f.root.clone();let before=fs::read(&path).unwrap();
    f.dacl(&root,"D:P(A;;FRFWFX;;;WD)");f.dacl(&path,"D:P(A;;GRGW;;;WD)");
    assert!(probe(&root,ADD_FILE).is_ok());assert_eq!(probe(&root,DELETE_CHILD).unwrap_err().raw_os_error(),Some(5));assert_eq!(probe(&path,DELETE).unwrap_err().raw_os_error(),Some(5));
    let error=replace_file(&path,"epicRecord").unwrap_err();
    assert!(error.contains("UM_ELEVATION_REQUIRED:")||crate::elevation::is_elevated());
    assert_error_path(&error,&path);assert_eq!(fs::read(&path).unwrap(),before);assert_eq!(fs::read_dir(&root).unwrap().count(),1);
}
#[cfg(windows)]
#[test]
fn sharing_violation_is_not_an_elevation_request(){
    use std::os::windows::fs::OpenOptionsExt;
    let f=Fixture::new();let path=f.file("busy");let _held=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
    let error=replace_file(&path,"engineFiles").unwrap_err();assert!(error.contains("32"));assert!(!error.contains("UM_ELEVATION_REQUIRED:"));
}

#[cfg(windows)]
#[test]
fn protected_registration_fails_before_engine_or_staged_files_are_changed(){
    let mut f=Fixture::new();let root=f.root.join("engine");let stage=f.root.join("stage");let records=f.root.join("records");
    fs::create_dir_all(root.join("Engine")).unwrap();fs::create_dir_all(stage.join("files/Engine")).unwrap();fs::create_dir(&records).unwrap();
    let old=root.join("Engine/file");let source=stage.join("files/Engine/file");fs::write(&old,b"old").unwrap();fs::write(&source,b"new").unwrap();
    let item=records.join("fixture.item");let before=serde_json::json!({"InstallLocation":root.to_string_lossy(),"AppVersionString":"old"});let item_bytes=serde_json::to_vec(&before).unwrap();fs::write(&item,&item_bytes).unwrap();
    let plan=Plan{resume_stage:Some(stage.clone()),catalog_item_id:None,remove_unselected:false,source_base:String::new(),info:Preview{id:uuid::Uuid::new_v4().to_string(),app_name:"UE_5.8".into(),build:"new".into(),path:root.to_string_lossy().into_owned(),update:true,components:vec![],bytes:3,download_bytes:0,changed:1,added:0,removed:0,required_bytes:0,free_bytes:1000000,conflicts:vec![]},raw:b"own manifest fixture".to_vec(),files:BTreeMap::new(),old:BTreeMap::new(),item:Some((item.clone(),before)),chunk_sizes:BTreeMap::new(),tags:BTreeMap::new()};
    let ops=[super::super::install::Operation{name:"Engine/file".into(),old_hash:Some(super::super::install::digest(b"old")),new_hash:Some(super::super::install::digest(b"new"))}];
    f.dacl(&records,"D:P(A;;FRFWFX;;;WD)");f.dacl(&item,"D:P(A;;GRGW;;;WD)");
    let error=super::super::install::permission_fixture_commit(&plan,&stage,&ops).unwrap_err();
    assert_error_path(&error,&item);assert!(!error.contains("Прежние файлы восстановлены"));
    assert_eq!(fs::read(&old).unwrap(),b"old");assert_eq!(fs::read(&source).unwrap(),b"new");assert_eq!(fs::read(&item).unwrap(),item_bytes);
    assert!(!stage.join("originals").exists());assert!(!stage.join("journal.json").exists());assert!(!root.join(".um-engine-update.json").exists());
    // The frozen 0.9.1 loop reaches the same protected record only after moving
    // files, then rolls back. The new preflight preserves assembled sources.
    let baseline=super::super::install::permission_baseline_commit(&plan,&stage,&ops).unwrap_err();
    assert!(baseline.contains("Прежние файлы восстановлены"));assert!(!source.exists());
    assert_eq!(fs::read(&old).unwrap(),b"old");assert_eq!(fs::read(&item).unwrap(),item_bytes);
}

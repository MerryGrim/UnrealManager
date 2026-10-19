use std::{fs,path::{Path,PathBuf}};
use serde::Serialize;
const PROG_ID:&str="UnrealManager.Project";
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct AssociationStatus { enabled:bool, windows_choice_required:bool }
fn config_path()->PathBuf {crate::storage::settings().join("project-association.json")}
fn config()->serde_json::Value {fs::read(config_path()).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_else(||serde_json::json!({}))}
#[tauri::command]
pub fn project_association_get()->AssociationStatus {
 let enabled=config()["enabled"].as_bool().unwrap_or(false);
 #[cfg(windows)] let windows_choice_required={use winreg::{RegKey,enums::*};RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.uproject\UserChoice").ok().and_then(|k|k.get_value::<String,_>("ProgId").ok()).is_some_and(|s|s!=PROG_ID)};
 #[cfg(not(windows))] let windows_choice_required=false;
 AssociationStatus{enabled,windows_choice_required}
}
#[tauri::command]
pub fn project_association_set(enabled:bool)->Result<AssociationStatus,String> {
 #[cfg(windows)] {
  use winreg::{RegKey,enums::*};
  let classes=RegKey::predef(HKEY_CURRENT_USER).create_subkey(r"Software\Classes").map_err(|e|e.to_string())?.0;
  let extension=classes.create_subkey(".uproject").map_err(|e|e.to_string())?.0;
  let mut options=config();
  let current=extension.get_value::<String,_>("").ok();
  if enabled {
   if !options["enabled"].as_bool().unwrap_or(false) { options["previous"]=current.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null); }
   // Save the previous association before changing it. Never delete Windows UserChoice.
   crate::storage::atomic(&config_path(),&serde_json::to_vec(&options).map_err(|e|e.to_string())?)?;
   let exe=std::env::current_exe().map_err(|e|e.to_string())?;
   let key=classes.create_subkey(PROG_ID).map_err(|e|e.to_string())?.0;
   key.set_value("",&"Unreal Engine Project — Unreal Manager").map_err(|e|e.to_string())?;
   key.create_subkey("DefaultIcon").map_err(|e|e.to_string())?.0.set_value("",&format!("\"{}\",0",exe.display())).map_err(|e|e.to_string())?;
   key.create_subkey(r"shell\open\command").map_err(|e|e.to_string())?.0.set_value("",&format!("\"{}\" --open-project \"%1\"",exe.display())).map_err(|e|e.to_string())?;
   extension.create_subkey("OpenWithProgids").map_err(|e|e.to_string())?.0.set_value(PROG_ID,&"").map_err(|e|e.to_string())?;
   extension.set_value("",&PROG_ID).map_err(|e|e.to_string())?;
  } else if current.as_deref()==Some(PROG_ID) {
   if let Some(previous)=options["previous"].as_str() {extension.set_value("",&previous).map_err(|e|e.to_string())?;}
   else {extension.delete_value("").map_err(|e|e.to_string())?;}
  }
  options["enabled"]=enabled.into();
  crate::storage::atomic(&config_path(),&serde_json::to_vec(&options).map_err(|e|e.to_string())?)?;
  unsafe {windows_sys::Win32::UI::Shell::SHChangeNotify(windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,windows_sys::Win32::UI::Shell::SHCNF_IDLIST,std::ptr::null(),std::ptr::null());}
  Ok(project_association_get())
 }
 #[cfg(not(windows))] {let _=enabled;Err("Поддерживается Windows".into())}
}
fn numeric_version(root:&Path)->Option<(u64,u64,u64,u64)> {
 let value:serde_json::Value=serde_json::from_slice(&fs::read(root.join("Engine/Build/Build.version")).ok()?).ok()?;
 Some((value["MajorVersion"].as_u64()?,value["MinorVersion"].as_u64()?,value["PatchVersion"].as_u64()?,value["Changelist"].as_u64().unwrap_or(0)))
}
fn latest(roots:impl IntoIterator<Item=PathBuf>)->Option<PathBuf> {
 roots.into_iter().filter(|p|!p.join(".um-engine-update.json").exists()&&p.join("Engine/Binaries/Win64/UnrealEditor.exe").is_file()).filter_map(|p|numeric_version(&p).map(|version|(version,p))).max_by(|a,b|a.0.cmp(&b.0).then_with(||a.1.cmp(&b.1))).map(|(_,p)|p)
}
pub fn open_latest(project:&Path)->Result<(),String> {
 if !project.is_absolute() || !project.is_file() || project.extension().and_then(|e|e.to_str()).is_none_or(|e|!e.eq_ignore_ascii_case("uproject")) {return Err("Файл проекта .uproject не найден".into());}
 let root=latest(crate::scan_engines().into_iter().map(|e|PathBuf::from(e.path))).ok_or("Не найден установленный Unreal Engine")?;
 let executable=root.join("Engine/Binaries/Win64/UnrealEditor.exe");
 std::process::Command::new(executable).arg(project).current_dir(root).spawn().map_err(|e|e.to_string())?;
 Ok(())
}
pub fn handle_args(app:&tauri::AppHandle,args:&[String]) {
 if args.get(1).map(String::as_str)!=Some("--open-project") {return;}
 let Some(path)=args.get(2).map(PathBuf::from) else{return;};
 let app=app.clone();
 std::thread::spawn(move||{if let Err(error)=open_latest(&path) {use tauri::{Emitter,Manager}; let _=app.emit("um-project-open-error",error.clone());if let Some(w)=app.get_webview_window("main"){let _=w.set_title(&format!("Unreal Manager — {error}"));}}});
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn latest_is_numeric_and_skips_pending(){
  let base=std::env::temp_dir().join(format!("um-latest-test-{}",uuid::Uuid::new_v4()));
  let mut roots=vec![];
  for (i,minor) in [9,10,8].iter().enumerate(){let p=base.join(i.to_string());fs::create_dir_all(p.join("Engine/Build")).unwrap();fs::create_dir_all(p.join("Engine/Binaries/Win64")).unwrap();fs::write(p.join("Engine/Binaries/Win64/UnrealEditor.exe"),b"fixture").unwrap();fs::write(p.join("Engine/Build/Build.version"),format!(r#"{{"MajorVersion":5,"MinorVersion":{minor},"PatchVersion":1}}"#)).unwrap();roots.push(p);}
  assert_eq!(latest(roots.clone()),Some(roots[1].clone()));fs::write(roots[1].join(".um-engine-update.json"),b"fixture").unwrap();assert_eq!(latest(roots.clone()),Some(roots[0].clone()));fs::remove_dir_all(base).unwrap();
 }
}

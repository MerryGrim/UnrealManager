use super::*;
#[derive(Default,Serialize,Deserialize)]
struct Receipt {system:Option<String>,completed:BTreeSet<String>}
#[cfg(all(windows,not(test)))]
fn system_identity()->Option<String>{
    use winreg::{enums::HKEY_LOCAL_MACHINE,RegKey};
    let key=RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography").ok()?;
    let guid:String=key.get_value("MachineGuid").ok()?;
    if guid.trim().is_empty(){return None;}
    // A copied portable data folder must not imply that redistributables are
    // installed in this Windows installation. Persist only an opaque digest.
    Some(install::digest(guid.trim().as_bytes()))
}
#[cfg(test)]fn system_identity()->Option<String>{Some("test-windows-installation".into())}
#[cfg(all(not(windows),not(test)))]fn system_identity()->Option<String>{None}
fn read_receipt(record:&Path,system:Option<&str>)->Result<BTreeSet<String>,String>{
    if !record.exists(){return Ok(BTreeSet::new());}
    let value:serde_json::Value=serde_json::from_slice(&fs::read(record).map_err(io)?).map_err(|_|"Повреждён список системных компонентов")?;
    // Legacy hash-only receipts carry no evidence about the current OS.
    if value.is_array(){return Ok(BTreeSet::new());}
    let receipt:Receipt=serde_json::from_value(value).map_err(|_|"Повреждён список системных компонентов")?;
    Ok(if system.is_some() && receipt.system.as_deref()==system {receipt.completed}else{BTreeSet::new()})
}
fn supported_files(files:&BTreeMap<String,FileManifestList>)->Vec<&'static str> {
    const BASE:&str="Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe";
    if files.contains_key(BASE) {return vec![BASE];}
    ["Engine/Extras/Redist/en-us/vc_redist.x64.exe","Engine/Extras/Redist/en-us/GameInputRedist.msi"].into_iter().filter(|p|files.contains_key(*p)).collect()
}
pub(super) fn install(plan:&Plan,suspend:Option<&AtomicBool>,progress:impl FnMut(&str,u64,u64))->Result<bool,String> {
    let root=target(&plan.info.path)?;
    let record=crate::storage::settings().join("engine-prerequisites.json");
    install_files(&root,&plan.files,&record,suspend,run,progress)
}
fn install_files(root:&Path,files:&BTreeMap<String,FileManifestList>,record:&Path,suspend:Option<&AtomicBool>,mut run_installer:impl FnMut(&Path)->Result<i32,String>,mut progress:impl FnMut(&str,u64,u64))->Result<bool,String> {
    let system=system_identity();
    install_files_for_system(root,files,record,suspend,system.as_deref(),&mut run_installer,&mut progress)
}
fn install_files_for_system(root:&Path,files:&BTreeMap<String,FileManifestList>,record:&Path,suspend:Option<&AtomicBool>,system:Option<&str>,mut run_installer:impl FnMut(&Path)->Result<i32,String>,mut progress:impl FnMut(&str,u64,u64))->Result<bool,String> {
    let installers=supported_files(files);
    no_links(&record)?;
    let mut done=read_receipt(record,system)?;
    let mut restart=false;
    progress("Проверяем системные компоненты",0,installers.len() as u64);
    for (i,name) in installers.iter().enumerate() {
        super::resume::check_suspend(suspend)?;
        progress("Проверяем системные компоненты",i as u64,installers.len() as u64);
        let file=root.join(relative(name)?);
        let expected=files[*name].file_hash.to_lowercase();
        if hash_file(&file)?!=expected {return Err("Установщик системных компонентов повреждён.".into());}
        super::resume::check_suspend(suspend)?;
        if !done.contains(&expected) {
            // An external installer has no trustworthy percentage. Do not kill it
            // on application shutdown: wait for its result, record success, then
            // observe suspension before starting another component.
            progress("Ожидаем завершения установки системных компонентов",i as u64,installers.len() as u64);
            let code=run_installer(&file)?;
            match code {0|1638=>(),3010|1641=>restart=true,_=>return Err(format!("Установка системных компонентов не завершена (код {code})."))}
            done.insert(expected);
            let receipt=Receipt{system:system.map(str::to_owned),completed:done.clone()};
            install::atomic(&record,&serde_json::to_vec(&receipt).map_err(|e|e.to_string())?)?;
        }
        progress("Проверяем системные компоненты",(i+1) as u64,installers.len() as u64);
        super::resume::check_suspend(suspend)?;
    }
    Ok(restart)
}
#[cfg(windows)]
fn run(file:&Path)->Result<i32,String> {
    use std::os::windows::process::CommandExt;
    let script=r#"$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$sig=Get-AuthenticodeSignature -LiteralPath $env:UM_PREREQ
if($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'Microsoft|Epic Games') {throw 'Invalid prerequisite signature'}
if([IO.Path]::GetExtension($env:UM_PREREQ) -eq '.msi') {
 $program=Join-Path $env:SystemRoot 'System32\msiexec.exe'
 $arguments=@('/i',('"'+$env:UM_PREREQ+'"'),'/passive','/norestart')
} else {
 $program=$env:UM_PREREQ
 $arguments=@('/install','/passive','/norestart')
}
$process=Start-Process -FilePath $program -ArgumentList $arguments -Verb RunAs -Wait -PassThru -WindowStyle Hidden
[Console]::Write($process.ExitCode)
"#;
    let output=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).env("UM_PREREQ",file).creation_flags(0x08000000).output().map_err(|e|e.to_string())?;
    if !output.status.success() {return Err("Системные компоненты не установлены: проверьте подпись установщика и подтвердите запрос Windows.".into());}
    String::from_utf8_lossy(&output.stdout).trim().parse().map_err(|_|"Не удалось получить результат установки системных компонентов.".into())
}
#[cfg(not(windows))]
fn run(_: &Path)->Result<i32,String>{Err("Поддерживается Windows".into())}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn uses_bundle_or_x64_components_without_arm64(){
  let mut files=BTreeMap::new();for n in ["vc_redist.x64.exe","vc_redist.arm64.exe","GameInputRedist.msi"]{files.insert(format!("Engine/Extras/Redist/en-us/{n}"),FileManifestList::default());}
  assert_eq!(supported_files(&files).len(),2);files.insert("Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe".into(),FileManifestList::default());assert_eq!(supported_files(&files),vec!["Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe"]);
 }
 #[test] fn records_finished_component_before_suspending_and_does_not_restart_it(){
  let base=std::env::temp_dir().join(format!("um-prereq-test-{}",uuid::Uuid::new_v4()));
  let mut files=BTreeMap::new();
  for (name,body) in [("vc_redist.x64.exe",b"first".as_slice()),("GameInputRedist.msi",b"second".as_slice())]{
   let name=format!("Engine/Extras/Redist/en-us/{name}");let path=base.join(&name);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,body).unwrap();
   files.insert(name,FileManifestList{file_hash:install::digest(body),..FileManifestList::default()});
  }
  let record=base.join("record.json");let suspend=AtomicBool::new(false);let mut calls=vec![];let mut events=vec![];
  let error=install_files(&base,&files,&record,Some(&suspend),|path|{calls.push(path.file_name().unwrap().to_string_lossy().into_owned());suspend.store(true,Ordering::SeqCst);Ok(0)},|phase,done,total|events.push((phase.to_string(),done,total))).unwrap_err();
  assert_eq!(error,super::super::resume::SUSPENDED);assert_eq!(calls,vec!["vc_redist.x64.exe"]);
  assert!(events.iter().any(|e|e==&("Ожидаем завершения установки системных компонентов".into(),0,2)));
  assert_eq!(events.last().unwrap(),&("Проверяем системные компоненты".into(),1,2));
  let saved:Receipt=serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();assert_eq!(saved.completed.len(),1);
  suspend.store(false,Ordering::SeqCst);calls.clear();
  assert!(install_files(&base,&files,&record,Some(&suspend),|path|{calls.push(path.file_name().unwrap().to_string_lossy().into_owned());Ok(3010)},|_,_,_|{}).unwrap());
  assert_eq!(calls,vec!["GameInputRedist.msi"]);
  let saved:Receipt=serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();assert_eq!(saved.completed.len(),2);
  fs::write(base.join("Engine/Extras/Redist/en-us/vc_redist.x64.exe"),b"corrupted").unwrap();
  assert!(install_files(&base,&files,&record,None,|_|panic!("corrupt component must not execute"),|_,_,_|{}).is_err());
  fs::remove_dir_all(base).unwrap();
 }
 #[test] fn copied_or_legacy_receipts_do_not_skip_components_on_a_new_windows_installation(){
  let base=std::env::temp_dir().join(format!("um-prereq-system-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&base).unwrap();
  let record=base.join("record.json");let receipt=Receipt{system:Some("previous-windows".into()),completed:BTreeSet::from(["known-hash".into()])};
  fs::write(&record,serde_json::to_vec(&receipt).unwrap()).unwrap();
  assert_eq!(read_receipt(&record,Some("previous-windows")).unwrap().len(),1);
  assert!(read_receipt(&record,Some("clean-windows")).unwrap().is_empty());
  assert!(read_receipt(&record,None).unwrap().is_empty());
  fs::write(&record,br#"["known-hash"]"#).unwrap();assert!(read_receipt(&record,Some("clean-windows")).unwrap().is_empty());
  assert!(base.is_absolute());assert!(base.file_name().unwrap().to_string_lossy().starts_with("um-prereq-system-test-"));no_links(&base).unwrap();fs::remove_dir_all(base).unwrap();
 }
}

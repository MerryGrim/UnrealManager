use super::*;
fn supported_files(files:&BTreeMap<String,FileManifestList>)->Vec<&'static str> {
    const BASE:&str="Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe";
    if files.contains_key(BASE) {return vec![BASE];}
    ["Engine/Extras/Redist/en-us/vc_redist.x64.exe","Engine/Extras/Redist/en-us/GameInputRedist.msi"].into_iter().filter(|p|files.contains_key(*p)).collect()
}
pub(super) fn install(plan:&Plan,mut progress:impl FnMut(u64,u64))->Result<bool,String> {
    let root=target(&plan.info.path)?;
    let installers=supported_files(&plan.files);
    let record=crate::storage::settings().join("engine-prerequisites.json");
    no_links(&record)?;
    let mut done:BTreeSet<String>=if record.exists(){serde_json::from_slice(&fs::read(&record).map_err(io)?).map_err(|_|"Повреждён список системных компонентов")?}else{BTreeSet::new()};
    let mut restart=false;
    for (i,name) in installers.iter().enumerate() {
        progress(i as u64,installers.len() as u64);
        let file=root.join(relative(name)?);
        let expected=plan.files[*name].file_hash.to_lowercase();
        if hash_file(&file)?!=expected {return Err("Установщик системных компонентов повреждён.".into());}
        if !done.contains(&expected) {
            let code=run(&file)?;
            match code {0|1638=>(),3010|1641=>restart=true,_=>return Err(format!("Установка системных компонентов не завершена (код {code})."))}
            done.insert(expected);
            install::atomic(&record,&serde_json::to_vec(&done).map_err(|e|e.to_string())?)?;
        }
        progress((i+1) as u64,installers.len() as u64);
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
}

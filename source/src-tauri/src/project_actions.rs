use std::{fs, path::{Path,PathBuf}, process::Command};

#[derive(Clone,Copy,serde::Deserialize)]
#[serde(rename_all="camelCase")]
pub enum DeleteMode { Recycle, Permanent }

#[cfg(windows)]
fn delete_flags(mode:DeleteMode)->u16 {
    use windows_sys::Win32::UI::Shell::*;
    match mode {
        DeleteMode::Recycle=>(FOF_ALLOWUNDO|FOF_NOCONFIRMATION|FOF_WANTNUKEWARNING) as u16,
        DeleteMode::Permanent=>FOF_NOCONFIRMATION as u16,
    }
}
pub(crate) fn recycle_folder(root:&Path)->Result<(),String>{
    #[cfg(windows)] {
        use windows_sys::Win32::UI::Shell::*;
        let raw=root.to_string_lossy();
        let path=if let Some(unc)=raw.strip_prefix(r"\\?\UNC\"){format!(r"\\{}",unc)}else{raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()};
        let source:Vec<u16>=path.encode_utf16().chain([0,0]).collect();
        let mut operation=SHFILEOPSTRUCTW::default();operation.wFunc=FO_DELETE;operation.pFrom=source.as_ptr();operation.fFlags=delete_flags(DeleteMode::Recycle);
        let code=unsafe{SHFileOperationW(&mut operation)};
        if operation.fAnyOperationsAborted!=0{return Err("Перемещение в корзину отменено".into());}
        if code!=0||root.exists(){return Err(format!("Не удалось переместить папку в корзину (код {code})"));}Ok(())
    }
    #[cfg(not(windows))] {Err("Корзина поддерживается только в Windows".into())}
}

fn descriptor(path:&str)->Result<PathBuf,String>{
    let p=PathBuf::from(path);
    if p.components().any(|c|matches!(c,std::path::Component::ParentDir)) || path.contains('\0'){return Err("Некорректный путь проекта".into());}
    if !p.is_absolute() || p.extension().and_then(|s|s.to_str()).map(|s|!s.eq_ignore_ascii_case("uproject")).unwrap_or(true) {return Err("Выберите файл .uproject".into());}
    if !p.is_file(){return Err("Файл проекта не найден".into());}
    Ok(p)
}

fn deletion_root(path:&str)->Result<PathBuf,String>{
    checked_deletion_root(path,&super::project_roots())
}

fn checked_deletion_root(path:&str, roots:&[PathBuf])->Result<PathBuf,String>{
    let p=descriptor(path)?;
    let root=p.parent().ok_or("Не найден каталог проекта")?;
    for ancestor in p.ancestors(){
        let meta=fs::symlink_metadata(ancestor).map_err(|e|e.to_string())?;
        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("Удаление проекта через ссылку не поддерживается".into());}}
        if meta.file_type().is_symlink(){return Err("Удаление проекта через ссылку не поддерживается".into());}
    }
    let canonical=root.canonicalize().map_err(|e|e.to_string())?;
    let allowed=roots.iter().filter_map(|r|r.canonicalize().ok()).any(|r|canonical!=r && canonical.starts_with(&r));
    if !allowed{return Err("Каталог проекта должен находиться внутри папки проектов".into());}
    for entry in walkdir::WalkDir::new(root).follow_links(false){
        let entry=entry.map_err(|e|e.to_string())?;
        let meta=fs::symlink_metadata(entry.path()).map_err(|e|e.to_string())?;
        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("В папке проекта есть ссылки. Удалите проект через Проводник".into());}}
        if meta.file_type().is_symlink(){return Err("В папке проекта есть ссылки".into());}
        if entry.path()!=p && entry.path().extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("uproject")) {return Err("В папке есть другой проект. Удаление всей папки заблокировано".into());}
    }
    Ok(root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(windows)]
    fn deletion_modes_are_explicit(){
        use windows_sys::Win32::UI::Shell::*;
        assert_ne!(delete_flags(DeleteMode::Recycle)&FOF_ALLOWUNDO as u16,0);
        assert_ne!(delete_flags(DeleteMode::Recycle)&FOF_WANTNUKEWARNING as u16,0);
        assert_eq!(delete_flags(DeleteMode::Permanent)&FOF_ALLOWUNDO as u16,0);
        assert!(serde_json::from_str::<DeleteMode>("\"unknown\"").is_err());
    }
    #[test]
    fn validates_delete_scope_without_deleting_user_files(){
        let root=std::env::temp_dir().join(format!("um-delete-test-{}",uuid::Uuid::new_v4()));
        let folder=root.join("Project");fs::create_dir_all(&folder).unwrap();
        let file=folder.join("Example.uproject");fs::write(&file,b"{}").unwrap();
        let roots=vec![root.clone()];
        assert_eq!(checked_deletion_root(file.to_str().unwrap(),&roots).unwrap(),folder);
        assert!(checked_deletion_root(file.to_str().unwrap(),&[folder.clone()]).is_err());
        assert!(checked_deletion_root(file.to_str().unwrap(),&[]).is_err());
        let second=folder.join("Other.uproject");fs::write(&second,b"{}").unwrap();
        assert!(checked_deletion_root(file.to_str().unwrap(),&roots).is_err());
        assert!(descriptor("../Example.uproject").is_err());
        assert!(file.exists());
        fs::remove_file(&second).unwrap();fs::remove_file(&file).unwrap();fs::remove_dir(&folder).unwrap();fs::remove_dir(&root).unwrap();
    }
}

#[tauri::command]
pub fn reveal_project(path:String)->Result<(),String>{
    let p=descriptor(&path)?;
    Command::new("explorer.exe").arg(p.parent().ok_or("Не найден каталог")?).spawn().map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn choose_project_location(folder:bool)->Result<Option<String>,String>{
    tauri::async_runtime::spawn_blocking(move||{
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            let script=r#"[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.Windows.Forms
if($env:UM_PICK_FOLDER -eq '1') {
 $dialog=New-Object System.Windows.Forms.FolderBrowserDialog
 $dialog.Description=$env:UM_PICK_TITLE
 $dialog.ShowNewFolderButton=$true
} else {
 $dialog=New-Object System.Windows.Forms.OpenFileDialog
 $dialog.Title=$env:UM_PICK_TITLE
 $dialog.Filter=$env:UM_PICK_FILTER
 $dialog.CheckFileExists=$true
}
try { if($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
 if($env:UM_PICK_FOLDER -eq '1') {[Console]::Write($dialog.SelectedPath)} else {[Console]::Write($dialog.FileName)}
}} finally {$dialog.Dispose()}
"#;
            let output=Command::new("powershell.exe").args(["-NoProfile","-STA","-NonInteractive","-Command",script]).env("UM_PICK_FOLDER",if folder{"1"}else{"0"}).env("UM_PICK_TITLE",crate::ui_locale::caption(if folder{"folder"}else{"project"})).env("UM_PICK_FILTER",crate::ui_locale::caption("filter")).creation_flags(0x08000000).output().map_err(|e|e.to_string())?;
            if !output.status.success(){return Err("Не удалось открыть окно выбора папки или проекта".into());}
            let path=String::from_utf8(output.stdout).map_err(|_|"Не удалось прочитать выбранный путь")?;
            Ok(if path.is_empty(){None}else{Some(path)})
        }
        #[cfg(not(windows))] {Err("Выбор папки поддерживается в Windows".into())}
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn project_delete_preview(path:String)->Result<String,String>{
    tauri::async_runtime::spawn_blocking(move|| deletion_root(&path).map(|p|p.to_string_lossy().into_owned())).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn delete_project(path:String, directory:String, mode:DeleteMode)->Result<(),String>{
    tauri::async_runtime::spawn_blocking(move||{
        let root=deletion_root(&path)?;
        if root!=Path::new(&directory){return Err("Каталог проекта изменился. Повторите подтверждение".into());}
        #[cfg(windows)] {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::UI::Shell::*;
            let source:Vec<u16>=root.as_os_str().encode_wide().chain([0,0]).collect();
            let mut operation=SHFILEOPSTRUCTW::default();
            operation.wFunc=FO_DELETE;operation.pFrom=source.as_ptr();
            operation.fFlags=delete_flags(mode);
            let code=unsafe{SHFileOperationW(&mut operation)};
            if operation.fAnyOperationsAborted!=0{return Err("Удаление отменено".into());}
            if code!=0{return Err(format!("Windows не смогла удалить проект (код {code})"));}
            if root.exists(){return Err("Папка проекта осталась на диске. Обновите список".into());}
            Ok(())
        }
        #[cfg(not(windows))] {Err("Удаление поддерживается только в Windows".into())}
    }).await.map_err(|e|e.to_string())?
}

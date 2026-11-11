//! Opt-in local export of explicit already-used/already-owned refusals only.
//! Raw keys remain in bounded zeroed native memory; snapshots expose a count.
use super::{Draft, Product, Reason, Secret, Status};
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use serde::Serialize;
use tauri::Manager;

pub(super) const UNAVAILABLE: &str = "Экспорт недоступен. Проверьте ключи заново.";
const SAVE_FAILED: &str = "Не удалось сохранить TXT. Выберите другое место и повторите.";
const TTL: Duration = Duration::from_secs(600);
const MAX_ROWS: usize = 100;
pub(super) type Shared = Arc<Mutex<Cache>>;
struct Row { item_id:String, key:Secret, title:String, reason:Reason }
pub(super) struct Cache { batch_id:String, account_id:String, rows:Vec<Row>, touched:Instant, valid:bool }
impl Cache {
    pub(super) fn shared(batch_id:&str)->Shared {Arc::new(Mutex::new(Self{batch_id:batch_id.into(),account_id:String::new(),rows:Vec::new(),touched:Instant::now(),valid:true}))}
    pub(super) fn clear(&mut self){self.rows.clear();self.account_id.clear();self.valid=false;}
    pub(super) fn matches(&self,batch:&str)->bool{self.batch_id==batch}
    fn available(&mut self,batch:&str,account:&str,at:Instant)->bool {
        if !self.valid||self.batch_id!=batch{return false;}
        if self.account_id!=account||account.is_empty()||at.saturating_duration_since(self.touched)>=TTL {self.clear();return false;}
        !self.rows.is_empty()
    }
    pub(super) fn touch(&mut self)->usize {if !self.valid{return 0;}self.touched=Instant::now();self.rows.len()}
}
pub(super) fn clear(shared:&Shared){if let Ok(mut cache)=shared.lock(){cache.clear();}}
pub(super) fn replace_slot(slot:&mut Option<Shared>,next:Shared){if let Some(previous)=slot.replace(next){clear(&previous);}}
pub(super) fn clear_slot(slot:&mut Option<Shared>){if let Some(previous)=slot.take(){clear(&previous);}}
pub(super) fn clear_batch(slot:&Option<Shared>,batch:&str){if let Some(cache)=slot{if cache.lock().is_ok_and(|cache|cache.matches(batch)){clear(cache);}}}
pub(super) fn clear_draft(draft:&mut Draft){clear(&draft.rejected);draft.snapshot.rejected_count=None;}
fn eligible(reason:Reason)->bool{matches!(reason,Reason::AlreadyUsed|Reason::AlreadyOwned)}
fn title_safe(title:&str,key:&str)->bool{
    !title.is_empty()&&title.chars().count()<=240&&!title.chars().any(char::is_control)
        &&!super::contains_ascii(&title.replace('-',""),&key.replace('-',""))
}
pub(super) fn remember(draft:&mut Draft,index:usize,reason:Reason,product:Option<&Product>){
    if !eligible(reason)||draft.snapshot.items[index].status!=Status::Failed{return;}
    let Some(key)=draft.keys[index].as_ref()else{return;};
    let item=&draft.snapshot.items[index];
    let title=if title_safe(&item.title,key.text()){item.title.clone()}
        else{product.filter(|p|super::supported(p)&&super::safe_title(p,key.text())).map(|p|p.title.clone()).unwrap_or_default()};
    let Ok(mut cache)=draft.rejected.lock()else{return;};
    if !cache.valid||cache.batch_id!=draft.snapshot.batch_id{return;}
    if cache.account_id.is_empty(){cache.account_id=draft.account_id.clone();}
    if cache.account_id!=draft.account_id||cache.account_id.is_empty(){cache.clear();return;}
    if let Some(row)=cache.rows.iter_mut().find(|row|row.item_id==item.id){row.reason=reason;if !title.is_empty(){row.title=title;}}
    else if cache.rows.len()<MAX_ROWS{cache.rows.push(Row{item_id:item.id.clone(),key:Secret(key.0.clone()),title,reason});}
    let count=cache.touch();draft.snapshot.rejected_count=(count>0).then_some(count);
}

#[derive(Serialize)]
pub(crate) struct ExportResult {saved:bool,count:usize}
struct ExportLease;
static EXPORTING:AtomicBool=AtomicBool::new(false);
impl Drop for ExportLease{fn drop(&mut self){EXPORTING.store(false,Ordering::SeqCst);}}
fn text(locale:&str)->[&'static str;5]{
    match locale {
        "ru"=>["Продукт\tКлюч\tПричина\r\n","Этот ключ уже использован.","Продукт уже есть в аккаунте.","Название не определено","Сохранить отклонённые ключи"],
        "de"=>["Produkt\tSchlüssel\tGrund\r\n","Dieser Schlüssel wurde bereits verwendet.","Das Produkt ist bereits im Konto vorhanden.","Produktname nicht verfügbar","Abgelehnte Schlüssel speichern"],
        "it"=>["Prodotto\tCodice\tMotivo\r\n","Questo codice è già stato utilizzato.","Il prodotto è già presente nell’account.","Nome del prodotto non disponibile","Salva i codici rifiutati"],
        "es"=>["Producto\tClave\tMotivo\r\n","Esta clave ya se ha utilizado.","El producto ya está en la cuenta.","Nombre del producto no disponible","Guardar claves rechazadas"],
        "fr"=>["Produit\tClé\tRaison\r\n","Cette clé a déjà été utilisée.","Le produit est déjà présent sur le compte.","Nom du produit indisponible","Enregistrer les clés refusées"],
        "zh"=>["产品\t密钥\t原因\r\n","此密钥已被使用。","账户已拥有此产品。","产品名称不可用","保存被拒绝的密钥"],
        "ja"=>["製品\tキー\t理由\r\n","このキーは使用済みです。","アカウントはこの製品を既に所有しています。","製品名は不明です","拒否されたキーを保存"],
        "ko"=>["제품\t키\t사유\r\n","이미 사용된 키입니다.","계정에 이미 있는 제품입니다.","제품 이름을 확인할 수 없음","거부된 키 저장"],
        _=>["Product\tKey\tReason\r\n","This key has already been used.","The product is already in the account.","Product name unavailable","Save rejected keys"],
    }
}
fn cell(value:&str)->String{
    let mut escaped=String::new();
    for ch in value.chars().take(240){match ch{'\\'=>escaped.push_str("\\\\"),'\t'=>escaped.push_str("\\t"),'\r'=>escaped.push_str("\\r"),'\n'=>escaped.push_str("\\n"),ch if ch.is_control()=>escaped.push(' '),ch=>escaped.push(ch)}}escaped
}
fn format(cache:&Cache,locale:&str)->Secret{
    let labels=text(locale);let mut bytes=Vec::with_capacity(256*1024);bytes.extend_from_slice(&[0xef,0xbb,0xbf]);bytes.extend_from_slice(labels[0].as_bytes());
    for row in &cache.rows{
        let title=cell(if row.title.is_empty(){labels[3]}else{&row.title});bytes.extend_from_slice(title.as_bytes());bytes.push(b'\t');
        bytes.extend_from_slice(&row.key.0);bytes.push(b'\t');bytes.extend_from_slice(labels[if row.reason==Reason::AlreadyUsed{1}else{2}].as_bytes());bytes.extend_from_slice(b"\r\n");
    }Secret(bytes)
}
fn save_selected(cache:&mut Cache,batch:&str,account:&str,selected:Option<&Path>,locale:&str,writer:impl FnOnce(&Path,&[u8])->Result<(),String>)->Result<ExportResult,String>{
    let Some(path)=selected else{return Ok(ExportResult{saved:false,count:0});};
    if !cache.available(batch,account,Instant::now()){return Err(UNAVAILABLE.into());}
    let bytes=format(cache,locale);writer(path,&bytes.0).map_err(|_|SAVE_FAILED.to_string())?;
    Ok(ExportResult{saved:true,count:cache.rows.len()})
}
fn write_atomic(path:&Path,bytes:&[u8])->Result<(),String>{
    use std::io::Write;
    write_atomic_with(path,bytes,|file,bytes|file.write_all(bytes))
}
fn write_atomic_with(path:&Path,bytes:&[u8],write:impl FnOnce(&mut std::fs::File,&[u8])->std::io::Result<()>)->Result<(),String>{
    use std::fs;
    if !path.is_absolute()||path.file_name().is_none()||!path.extension().is_some_and(|extension|extension.eq_ignore_ascii_case("txt")){return Err(SAVE_FAILED.into());}
    super::super::vault::no_links(path).map_err(|_|SAVE_FAILED)?;
    let parent=path.parent().ok_or(SAVE_FAILED)?;if !parent.is_dir(){return Err(SAVE_FAILED.into());}
    let temp=parent.join(format!(".um-rejected-{}.tmp",uuid::Uuid::new_v4()));
    super::super::vault::no_links(&temp).map_err(|_|SAVE_FAILED)?;
    let mut options=fs::OpenOptions::new();options.write(true).create_new(true);
    // Keep the uncommitted file unreadable to other opens while permitting
    // atomic rename with this handle held. Do not leave readable key bytes in
    // a failed temporary even if a later filesystem cleanup is obstructed.
    #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE);}
    let mut file=options.open(&temp).map_err(|_|SAVE_FAILED)?;
    let written=write(&mut file,bytes).and_then(|_|file.sync_all());
    let result=written.map_err(|_|SAVE_FAILED.to_string()).and_then(|_|{super::super::vault::no_links(path).map_err(|_|SAVE_FAILED)?;fs::rename(&temp,path).map_err(|_|SAVE_FAILED.into())});
    if result.is_err(){let _=file.set_len(0);let _=file.sync_all();}drop(file);
    if result.is_err(){let _=fs::remove_file(&temp);}result
}

#[cfg(windows)]
fn choose_path(owner:isize,locale:&str)->Result<Option<PathBuf>,String>{
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let filename=format!("UnrealManager-Rejected-Keys-{}.txt",chrono::Local::now().format("%Y-%m-%d"));
    let mut buffer=vec![0u16;32768];let name:Vec<u16>=filename.encode_utf16().collect();buffer[..name.len()].copy_from_slice(&name);
    let filter:Vec<u16>="Text files (*.txt)\0*.txt\0\0".encode_utf16().collect();let extension:Vec<u16>="txt\0".encode_utf16().collect();let title:Vec<u16>=text(locale)[4].encode_utf16().chain([0]).collect();
    let mut dialog=OPENFILENAMEW{lStructSize:std::mem::size_of::<OPENFILENAMEW>()as u32,hwndOwner:owner as _,lpstrFilter:filter.as_ptr(),nFilterIndex:1,lpstrFile:buffer.as_mut_ptr(),nMaxFile:buffer.len()as u32,lpstrTitle:title.as_ptr(),lpstrDefExt:extension.as_ptr(),Flags:OFN_EXPLORER|OFN_PATHMUSTEXIST|OFN_OVERWRITEPROMPT|OFN_NOCHANGEDIR|OFN_DONTADDTORECENT|OFN_NOTESTFILECREATE,..Default::default()};
    if unsafe{GetSaveFileNameW(&mut dialog)}==0{return if unsafe{CommDlgExtendedError()}==0{Ok(None)}else{Err(SAVE_FAILED.into())};}
    let end=buffer.iter().position(|v|*v==0).ok_or(SAVE_FAILED)?;
    use std::os::windows::ffi::OsStringExt;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..end]))))
}
async fn save_dialog(app:&tauri::AppHandle)->Result<Option<PathBuf>,String>{
    #[cfg(windows)]{
        let owner=app.get_webview_window("main").ok_or(UNAVAILABLE)?.hwnd().map_err(|_|UNAVAILABLE)?.0 as isize;
        let locale=crate::ui_locale::language();let(tx,rx)=tokio::sync::oneshot::channel();
        app.run_on_main_thread(move||{let _=tx.send(choose_path(owner,locale));}).map_err(|_|SAVE_FAILED)?;
        rx.await.map_err(|_|SAVE_FAILED)?
    }
    #[cfg(not(windows))]{let _=app;Err(SAVE_FAILED.into())}
}
pub(super) async fn export(app:tauri::AppHandle,state:&super::super::epic::EpicState,batch_id:String)->Result<ExportResult,String>{
    let _operation=crate::app_updates::operation()?;
    if uuid::Uuid::parse_str(&batch_id).is_err(){return Err(UNAVAILABLE.into());}
    EXPORTING.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).map_err(|_|UNAVAILABLE)?;let _lease=ExportLease;
    let shared={let store=super::registry().lock().map_err(|_|UNAVAILABLE)?;store.rejected.clone().ok_or(UNAVAILABLE)?};
    let account=match super::super::epic::cached_account_binding(state).await{Some(account)=>account,None=>{clear(&shared);return Err(UNAVAILABLE.into());}};
    {let mut cache=shared.lock().map_err(|_|UNAVAILABLE)?;if !cache.available(&batch_id,&account,Instant::now()){return Err(UNAVAILABLE.into());}}
    let selected=save_dialog(&app).await?;
    if selected.is_none(){return Ok(ExportResult{saved:false,count:0});}
    // Re-read the in-memory account after the user's potentially long dialog.
    // Login/logout clears this same cache before replacing the account, and
    // cannot interleave with the short serialization/atomic-write lock below.
    let account=match super::super::epic::cached_account_binding(state).await{Some(account)=>account,None=>{clear(&shared);return Err(UNAVAILABLE.into());}};
    let shared_copy=shared.clone();let locale=crate::ui_locale::language();
    tauri::async_runtime::spawn_blocking(move||{let mut cache=shared_copy.lock().map_err(|_|UNAVAILABLE)?;save_selected(&mut cache,&batch_id,&account,selected.as_deref(),locale,write_atomic)}).await.map_err(|_|SAVE_FAILED)?
}

#[cfg(test)]
#[path="epic_redeem_rejected_tests.rs"]
mod tests;

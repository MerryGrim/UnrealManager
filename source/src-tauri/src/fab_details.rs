use serde::{Deserialize,Serialize};
use serde_json::Value;
use sha1::{Digest,Sha1};
use std::{fs,path::PathBuf,time::{Duration,SystemTime,UNIX_EPOCH}};
use crate::epic::{EpicState,library_item};

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Details {
    id:String, description:String, author:String, images:Vec<String>, url:Option<String>,
    cached_at:u64, #[serde(default)] pub notice:String,
}
fn now()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}
fn path(id:&str)->PathBuf {crate::storage::cache().join("details").join(format!("{:x}.json",Sha1::digest(id.as_bytes())))}
fn read(id:&str)->Option<Details>{let p=path(id);if fs::metadata(&p).ok()?.len()>4*1024*1024{return None;}let d:Details=serde_json::from_slice(&fs::read(p).ok()?).ok()?;if d.id==id{Some(d)}else{None}}
fn save(d:&Details)->Result<(),String>{let p=path(&d.id);fs::create_dir_all(p.parent().unwrap()).map_err(|_|"Не удалось создать кэш информации")?;let tmp=p.with_extension(format!("{}.tmp",uuid::Uuid::new_v4()));fs::write(&tmp,serde_json::to_vec(d).map_err(|_|"Не удалось сохранить информацию")?).map_err(|_|"Не удалось записать кэш информации")?;fs::rename(tmp,p).map_err(|_|"Не удалось обновить кэш информации".into())}
fn listing_url(value:&str)->Option<(String,String)>{
    let u=reqwest::Url::parse(value).ok()?;
    if u.scheme()!="https" || !matches!(u.host_str(),Some("fab.com"|"www.fab.com")) || u.port_or_known_default()!=Some(443){return None;}
    let parts:Vec<_>=u.path_segments()?.collect();let i=parts.iter().position(|p|*p=="listings")?;
    let id=uuid::Uuid::parse_str(parts.get(i+1)?).ok()?.to_string();Some((format!("https://www.fab.com/listings/{id}"),id))
}
fn images(urls:impl Iterator<Item=String>)->Vec<String>{let mut result=Vec::new();for url in urls{if result.len()==40{break;}if reqwest::Url::parse(&url).is_ok_and(|u|crate::fab_cache::image_allowed(&u))&&!result.contains(&url){result.push(url);}}result}
fn enrich(d:&mut Details,v:&Value,uid:&str)->Result<(),String>{
    if v["uid"].as_str()!=Some(uid){return Err("Fab вернул другую карточку".into());}
    if let Some(s)=v["description"].as_str().filter(|s|!s.trim().is_empty()){d.description=s.into();}
    d.author=v["user"]["sellerName"].as_str().unwrap_or("").into();
    d.images=images(d.images.clone().into_iter().chain(v["thumbnails"].as_array().into_iter().flatten().filter_map(|i|i["mediaUrl"].as_str().map(str::to_owned))));Ok(())
}
#[tauri::command]
pub async fn fab_details(state:tauri::State<'_,EpicState>,id:String,refresh:bool)->Result<Details,String>{
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    let item=library_item(&state,&id).await?;
    let cached=read(&id);
    let link=listing_url(&item.listing_url);
    if !refresh {if let Some(d)=cached.as_ref().filter(|d|item.info_version>0 && d.url==link.as_ref().map(|l|l.0.clone()) && now().saturating_sub(d.cached_at)<if d.notice.is_empty(){86400}else{300}){return Ok(d.clone());}}
    let item_json=serde_json::to_value(&item).map_err(|_|"Не удалось прочитать материал")?;
    let mut d=Details{id:id.clone(),description:item_json["description"].as_str().unwrap_or("").into(),author:String::new(),images:images(item.images.into_iter().chain(item_json["cover"].as_str().map(str::to_owned))),url:link.as_ref().map(|l|l.0.clone()),cached_at:now(),notice:String::new()};
    if let Some((_,uid))=link {
        let fetched=async {
            let client=reqwest::Client::builder().timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"Нет соединения с Fab")?;
            let mut response=client.get(format!("https://www.fab.com/i/listings/{uid}")).send().await.map_err(|_|"Fab недоступен")?;
            if !response.status().is_success(){return Err(format!("Fab не предоставил подробности (HTTP {})",response.status().as_u16()));}
            let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|"Ответ Fab прерван")?{if bytes.len()+chunk.len()>2*1024*1024{return Err("Ответ Fab слишком большой".into());}bytes.extend_from_slice(&chunk);}
            let value:Value=serde_json::from_slice(&bytes).map_err(|_|"Не удалось прочитать ответ Fab")?;enrich(&mut d,&value,&uid)
        }.await;
        if let Err(e)=fetched {d.notice=format!("{e}. Показаны сведения из библиотеки.");}
    } else {d.notice=if item.info_version<1 {"Подробности появятся после синхронизации библиотеки."}else{"Показаны сведения из библиотеки. Ссылка на карточку Fab отсутствует."}.into();}
    if !d.notice.is_empty(){
        if let Ok(v)=crate::epic::library_metadata(&state,&id).await {
            if let Some(text)=v["longDescription"].as_str().or(v["description"].as_str()).filter(|s|!s.is_empty()){d.description=text.into();}
            if let Some(text)=v["technicalDetails"].as_str().filter(|s|!s.is_empty()){d.description.push_str("\n\nТехнические сведения\n");d.description.push_str(text);}
            d.author=v["developer"].as_str().unwrap_or("").into();
            d.images=images(d.images.into_iter().chain(v["keyImages"].as_array().into_iter().flatten().filter_map(|i|i["url"].as_str().map(str::to_owned))));
            d.notice="Расширенные сведения получены из каталога Epic.".into();
        }else if let Some(mut old)=cached.filter(|old|old.description.len()>d.description.len()||old.images.len()>d.images.len()) {old.notice="Сервис не предоставил обновление. Показана сохранённая информация.".into();return Ok(old);}
    }
    if let Err(e)=save(&d){d.notice=if d.notice.is_empty(){e}else{format!("{} {e}",d.notice)};}Ok(d)
}
#[tauri::command]
pub async fn fab_detail_image(state:tauri::State<'_,EpicState>,id:String,index:usize)->Result<String,String>{
    let _cache_guard=crate::storage::CACHE_ACCESS.read().await;
    library_item(&state,&id).await?;
    let d=read(&id).ok_or("Сначала откройте информацию о материале")?;
    let url=d.images.get(index).ok_or("Изображение недоступно")?.clone();
    crate::fab_cache::cached_image(format!("{id}:{url}"),url,d.cached_at,true).await
}
#[tauri::command]
pub async fn fab_open_listing(state:tauri::State<'_,EpicState>,id:String)->Result<(),String>{
    let item=library_item(&state,&id).await?;let (url,_)=listing_url(&item.listing_url).ok_or("Ссылка на Fab отсутствует")?;
    #[cfg(windows)] {use std::os::windows::ffi::OsStrExt;let target:Vec<u16>=std::ffi::OsStr::new(&url).encode_wide().chain([0]).collect();let verb:Vec<u16>="open".encode_utf16().chain([0]).collect();let r=unsafe{windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(),verb.as_ptr(),target.as_ptr(),std::ptr::null(),std::ptr::null(),1)} as isize;if r<=32{return Err("Не удалось открыть браузер".into());}Ok(())}
    #[cfg(not(windows))] {let _=url;Err("Открытие браузера поддерживается в Windows".into())}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]fn exact_listing_and_image_origins(){let id="12345678-1234-1234-1234-123456789abc";assert!(listing_url(&format!("https://www.fab.com/en-US/listings/{id}")).is_some());assert!(listing_url(&format!("https://www.fab.com.evil.test/listings/{id}")).is_none());assert!(listing_url("https://www.fab.com/listings/../x").is_none());assert_eq!(images(vec!["https://cdn1.epicgames.com/a.jpg".into(),"https://cdn1.epicgames.com/a.jpg".into(),"http://localhost/a.jpg".into()].into_iter()).len(),1);}
    #[test]fn mismatched_listing_never_replaces_description(){let mut d=Details{id:"asset".into(),description:"library".into(),author:String::new(),images:vec![],url:None,cached_at:0,notice:String::new()};assert!(enrich(&mut d,&serde_json::json!({"uid":"other","description":"wrong"}),"right").is_err());assert_eq!(d.description,"library");}
}

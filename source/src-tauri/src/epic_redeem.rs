use std::sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}};
use serde::{Serialize,Deserialize};
use tauri::{Manager,WebviewUrl,WebviewWindowBuilder,WindowEvent};
const LABEL:&str="epic-redeem";
static ACTIVE:AtomicBool=AtomicBool::new(false);
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RedeemResult {status:String,title:String,error_code:String}
fn result(url:&tauri::Url,nonce:&str)->Option<RedeemResult>{
    if url.scheme()!="https" || url.host_str()!=Some("localhost") || url.port_or_known_default()!=Some(443) || url.path()!=format!("/um-redeem/{nonce}"){return None;}
    let raw=url.query_pairs().find(|(k,_)|k=="result")?.1;
    if raw.len()>3000{return None;}
    let mut value:RedeemResult=serde_json::from_str(&raw).ok()?;
    if !["ready","success","failed","unknown"].contains(&value.status.as_str()){return None;}
    value.title=value.title.chars().filter(|c|!c.is_control()).take(240).collect();
    if !value.error_code.starts_with("errors.com.epicgames.") || !value.error_code.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_'){value.error_code.clear();}
    Some(value)
}
#[tauri::command]
pub async fn epic_open_redeem(app:tauri::AppHandle,state:tauri::State<'_,super::epic::EpicState>,on_result:tauri::ipc::Channel<RedeemResult>)->Result<(),String>{
    if ACTIVE.swap(true,Ordering::SeqCst){if let Some(w)=app.get_webview_window(LABEL){let _=w.set_focus();}return Err("Окно активации уже открыто".into());}
    struct Guard;impl Drop for Guard{fn drop(&mut self){ACTIVE.store(false,Ordering::SeqCst);}}let _guard=Guard;
    let remember=super::epic::remember_web_session(&app,&state).await?;
    let main=app.get_webview_window("main").ok_or("Главное окно недоступно")?;
    let nonce=uuid::Uuid::new_v4().simple().to_string();
    let callback=serde_json::to_string(&format!("https://localhost/um-redeem/{nonce}")).unwrap();
    let script=include_str!("redeem_observer.js").replace("__CALLBACK__",&callback);
    let (tx,rx)=tokio::sync::oneshot::channel();let tx=Arc::new(Mutex::new(Some(tx)));
    // Channel::drop evaluates JS. Never own it in a native window callback:
    // WebView2 releases navigation handlers while Tauri borrows its window map.
    let (results_tx, mut results_rx) = tokio::sync::mpsc::unbounded_channel::<RedeemResult>();
    let relay = tauri::async_runtime::spawn(async move {
        while let Some(value) = results_rx.recv().await {
            let _ = on_result.send(value);
        }
    });
    let window=WebviewWindowBuilder::new(&app,LABEL,WebviewUrl::External("https://store.epicgames.com/redeem?lang=ru".parse().unwrap()))
        .title(crate::ui_locale::caption("redeem"))
        .owner(&main).map_err(|_|"Не удалось связать окно активации с главным окном")?
        .data_directory(super::epic_browser::profile(&app)?)
        .incognito(!remember)
        .inner_size(940.0,760.0).min_inner_size(640.0,560.0).center()
        .initialization_script(&script)
        .on_navigation(move|url|{
            if url.host_str()==Some("localhost"){
                if let Some(value)=result(url,&nonce){let _=results_tx.send(value);}return false;
            }
            url.scheme()=="https" && url.host_str()!=Some("tauri.localhost") && !url.host_str().is_some_and(|h|h.parse::<std::net::IpAddr>().is_ok())
        }).build().map_err(|_|"Не удалось открыть окно активации")?;
    window.on_window_event(move|event|{if matches!(event,WindowEvent::Destroyed){if let Ok(mut pending)=tx.lock(){if let Some(tx)=pending.take(){let _=tx.send(());}}}});
    let _=rx.await;
    let _=relay.await;
    super::epic_browser::restore_main(&app);
    Ok(())
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn callback_is_bound_to_window(){
        let mut url:tauri::Url="https://localhost/um-redeem/current".parse().unwrap();
        url.query_pairs_mut().append_pair("result",r#"{"status":"success","title":"Test Product","errorCode":""}"#);
        assert_eq!(result(&url,"current").unwrap().title,"Test Product");assert!(result(&url,"old").is_none());
        url.set_host(Some("localhost.evil")).unwrap();assert!(result(&url,"current").is_none());
    }
}

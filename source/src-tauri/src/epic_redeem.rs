use std::sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}};
use serde::{Serialize,Deserialize};
use tauri::{Manager,WebviewUrl,WebviewWindowBuilder,WindowEvent};
const LABEL:&str="epic-redeem";
const REDEEM_URL:&str="https://store.epicgames.com/redeem";
const EXCHANGE_URL:&str="https://www.epicgames.com/id/exchange";
static ACTIVE:AtomicBool=AtomicBool::new(false);
pub(crate) fn is_open()->bool{ACTIVE.load(Ordering::SeqCst)}

fn redemption_url(language:&str,exchange_code:Option<&str>)->Result<tauri::Url,String>{
    // App languages are resolved from the persisted preference (including
    // "system") before rendering. Never accept a URL or arbitrary locale here.
    let locale=match language {
        "ru"=>"ru","de"=>"de","it"=>"it","es"=>"es-ES","fr"=>"fr",
        "zh"=>"zh-CN","ja"=>"ja","ko"=>"ko",_=>"en-US",
    };
    let mut redeem:tauri::Url=REDEEM_URL.parse().map_err(|_|"Не удалось открыть окно активации")?;
    redeem.query_pairs_mut().append_pair("lang",locale);
    let Some(code)=exchange_code else { return Ok(redeem); };
    // Accept only a single opaque code, never a URL, bearer token or supplied
    // redirect. Errors intentionally contain no part of the received value.
    if !(16..=512).contains(&code.len()) || !code.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_') {
        return Err("Не удалось открыть окно активации".into());
    }
    let mut url:tauri::Url=EXCHANGE_URL.parse().map_err(|_|"Не удалось открыть окно активации")?;
    url.query_pairs_mut().append_pair("exchangeCode",code).append_pair("redirectUrl",redeem.as_str());
    Ok(url)
}
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
    if ACTIVE.load(Ordering::SeqCst){if let Some(w)=app.get_webview_window(LABEL){let _=w.set_focus();}return Err("Окно активации уже открыто".into());}
    let _web_flow=super::epic::reserve_web_flow()?;
    ACTIVE.store(true,Ordering::SeqCst);
    struct Guard;impl Drop for Guard{fn drop(&mut self){ACTIVE.store(false,Ordering::SeqCst);}}let _guard=Guard;
    let session=super::epic::prepare_web_session(&app,&state).await?;
    let url=redemption_url(crate::ui_locale::language(),session.exchange_code.as_deref())?;
    let remember=session.remember;
    drop(session);
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
    let window=WebviewWindowBuilder::new(&app,LABEL,WebviewUrl::External(url))
        .additional_browser_args(crate::webview_policy::BROWSER_ARGUMENTS)
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
    if crate::app_updates::applying(){let _=window.destroy();return Err("Не удалось открыть окно активации".into());}
    window.on_window_event(move|event|{if matches!(event,WindowEvent::Destroyed){if let Ok(mut pending)=tx.lock(){if let Some(tx)=pending.take(){let _=tx.send(());}}}});
    let _=rx.await;
    let _=relay.await;
    super::epic_browser::restore_main(&app);
    Ok(())
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn signed_out_uses_official_redemption_without_handoff(){
        assert_eq!(redemption_url("ru",None).unwrap().as_str(),"https://store.epicgames.com/redeem?lang=ru");
    }
    #[test]fn handoff_is_bound_to_exact_epic_origin_and_redemption_target(){
        let code="synthetic_Exchange-Code0123456789";
        let url=redemption_url("ru",Some(code)).unwrap();
        assert_eq!(url.scheme(),"https");assert_eq!(url.host_str(),Some("www.epicgames.com"));
        assert_eq!(url.port_or_known_default(),Some(443));assert_eq!(url.path(),"/id/exchange");
        assert_eq!(url.username(),"");assert!(url.password().is_none());assert!(url.fragment().is_none());
        let pairs:Vec<_>=url.query_pairs().collect();assert_eq!(pairs.len(),2);
        assert_eq!(pairs[0],("exchangeCode".into(),code.into()));
        assert_eq!(pairs[1],("redirectUrl".into(),"https://store.epicgames.com/redeem?lang=ru".into()));
        assert!(!url.as_str().contains("access_token"));assert!(!url.as_str().contains("refresh_token"));
    }
    #[test]fn handoff_rejects_redirect_injection_and_never_echoes_credentials(){
        for value in ["", "short", "syntheticCode123456&redirectUrl=https://evil.invalid", "https://evil.invalid/syntheticCode123456", "syntheticCode123456\r\nHeader: value", "eg1~syntheticBearerToken012345", "syntheticCode123456#fragment", "синтетическийКод123456"]{
            let error=redemption_url("en",Some(value)).unwrap_err();
            if !value.is_empty(){assert!(!error.contains(value));}
        }
        assert!(redemption_url("en",Some(&"a".repeat(513))).is_err());
    }
    #[test]fn all_ui_languages_use_the_same_localized_target_with_or_without_handoff(){
        for (language,locale) in [("ru","ru"),("en","en-US"),("de","de"),("it","it"),("es","es-ES"),("fr","fr"),("zh","zh-CN"),("ja","ja"),("ko","ko")] {
            let target=redemption_url(language,None).unwrap();
            assert_eq!(target.scheme(),"https");assert_eq!(target.host_str(),Some("store.epicgames.com"));
            assert_eq!(target.path(),"/redeem");assert_eq!(target.port_or_known_default(),Some(443));
            assert_eq!(target.username(),"");assert!(target.password().is_none());assert!(target.fragment().is_none());
            assert_eq!(target.query_pairs().collect::<Vec<_>>(),vec![("lang".into(),locale.into())]);
            let code="synthetic_Exchange-Code0123456789";
            let handoff=redemption_url(language,Some(code)).unwrap();
            assert_eq!(handoff.host_str(),Some("www.epicgames.com"));assert_eq!(handoff.path(),"/id/exchange");
            assert_eq!(handoff.query_pairs().collect::<Vec<_>>(),vec![("exchangeCode".into(),code.into()),("redirectUrl".into(),target.as_str().into())]);
        }
    }
    #[test]fn unsupported_or_untrusted_locale_falls_back_without_changing_the_target(){
        let expected=redemption_url("en",None).unwrap();
        for language in ["","unknown","system","en-US","ru&redirectUrl=https://evil.invalid","https://evil.invalid","zh\r\nHeader:value"] {
            assert_eq!(redemption_url(language,None).unwrap(),expected);
            let handoff=redemption_url(language,Some("synthetic_Code0123456789")).unwrap();
            let pairs:Vec<_>=handoff.query_pairs().collect();
            assert_eq!(pairs.len(),2);assert_eq!(pairs[1],("redirectUrl".into(),expected.as_str().into()));
        }
    }
    #[test]fn handoff_accepts_only_the_bounded_opaque_code(){
        for size in [16,512] {assert!(redemption_url("zh",Some(&"a".repeat(size))).is_ok());}
        for size in [0,15,513] {assert!(redemption_url("zh",Some(&"a".repeat(size))).is_err());}
    }
    #[test]fn callback_is_bound_to_window(){
        let mut url:tauri::Url="https://localhost/um-redeem/current".parse().unwrap();
        url.query_pairs_mut().append_pair("result",r#"{"status":"success","title":"Test Product","errorCode":""}"#);
        assert_eq!(result(&url,"current").unwrap().title,"Test Product");assert!(result(&url,"old").is_none());
        url.set_host(Some("localhost.evil")).unwrap();assert!(result(&url,"current").is_none());
    }
}

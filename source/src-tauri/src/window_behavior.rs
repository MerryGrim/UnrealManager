use std::sync::{LazyLock, Mutex, atomic::{AtomicBool,Ordering}};
use tauri::{Emitter, Manager};
static EXITING:AtomicBool=AtomicBool::new(false);
static BEHAVIOR: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(
    std::fs::read(crate::storage::settings().join("window.json")).ok()
        .and_then(|v| serde_json::from_slice::<String>(&v).ok())
        .filter(|v| valid(v)).unwrap_or_else(|| "ask".into())
));
fn valid(value:&str)->bool {matches!(value,"ask"|"tray"|"exit")}
#[tauri::command]
pub fn close_behavior_get()->String {BEHAVIOR.lock().unwrap_or_else(|e|e.into_inner()).clone()}
#[tauri::command]
pub fn close_behavior_set(behavior:String)->Result<(),String> {
    if !valid(&behavior) {return Err("Invalid close behavior".into());}
    let mut saved=BEHAVIOR.lock().map_err(|e|e.to_string())?;
    crate::engine::install::atomic(&crate::storage::settings().join("window.json"),&serde_json::to_vec(&behavior).map_err(|e|e.to_string())?)?;
    *saved=behavior;Ok(())
}
fn active(app:&tauri::AppHandle)->bool {
    let vault=app.state::<crate::vault::VaultState>();
    vault.active.load(Ordering::SeqCst)||vault.updating.load(Ordering::SeqCst)||app.state::<crate::engine::EngineState>().active.load(Ordering::SeqCst)
}
pub fn show(app:&tauri::AppHandle) {
    if let Some(window)=app.get_webview_window("main") {let _=window.show();let _=window.unminimize();let _=window.set_focus();}
}
#[tauri::command]
pub async fn window_close_action(app:tauri::AppHandle,action:String,remember:bool)->Result<(),String> {
    if !matches!(action.as_str(),"tray"|"exit") {return Err("Invalid close action".into());}
    if action=="tray" && app.tray_by_id("um-tray").is_none() {return Err("Не удалось создать значок в трее.".into());}
    if remember {close_behavior_set(action.clone())?;}
    if action=="tray" {app.get_webview_window("main").ok_or("Main window missing")?.hide().map_err(|e|e.to_string())?;}
    else {
        if EXITING.swap(true,Ordering::SeqCst){return Ok(());}
        crate::vault_queue::stop_dispatch(&app);
        let _=crate::epic_login::epic_cancel_login(app.clone());
        stop_operations(&app.state::<crate::engine::EngineState>(),&app.state::<crate::vault::VaultState>()).await;
        app.exit(0);
    }
    Ok(())
}
async fn stop_operations(engine:&crate::engine::EngineState,vault:&crate::vault::VaultState){
    while engine.active.load(Ordering::SeqCst)||vault.active.load(Ordering::SeqCst)||vault.updating.load(Ordering::SeqCst)||crate::app_updates::operations_active(){
        engine.suspend.store(true,Ordering::SeqCst);engine.cancel.store(true,Ordering::SeqCst);engine.paused.store(false,Ordering::SeqCst);vault.cancel.store(true,Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
pub(crate) async fn prepare_update(app:&tauri::AppHandle)->Result<(),String> {
    if EXITING.swap(true,Ordering::SeqCst) { return Err("Программа уже завершает работу.".into()); }
    crate::vault_queue::stop_dispatch(app);
    let stop=async {
        crate::epic_login::epic_cancel_login(app.clone())?;
        stop_operations(&app.state::<crate::engine::EngineState>(),&app.state::<crate::vault::VaultState>()).await;
        Ok::<(),String>(())
    };
    let result=tokio::time::timeout(std::time::Duration::from_secs(120),stop).await;
    if !matches!(result,Ok(Ok(()))) {
        EXITING.store(false,Ordering::SeqCst);
        crate::vault_queue::resume_dispatch(app);
        if let Ok(Err(error))=result { return Err(error); }
        return Err("Не удалось безопасно остановить операции. Повторите обновление после их завершения.".into());
    }
    Ok(())
}
pub(crate) fn abort_update(app:&tauri::AppHandle) { EXITING.store(false,Ordering::SeqCst);crate::vault_queue::resume_dispatch(app); }
pub fn request_close(app:&tauri::AppHandle) {
    if EXITING.load(Ordering::SeqCst){return;}
    let behavior=close_behavior_get();
    if behavior=="ask" {let _=app.emit("um-close-request", "");}
    else {let handle=app.clone();tauri::async_runtime::spawn(async move{if behavior=="exit"&&active(&handle){show(&handle);let _=handle.emit("um-close-request","__closing__");}if let Err(error)=window_close_action(handle.clone(),behavior,false).await{show(&handle);let _=handle.emit("um-close-request",error);}});}
}
fn menu(app:&tauri::AppHandle)->tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let open=tauri::menu::MenuItem::with_id(app,"um-open",crate::ui_locale::caption("tray_open"),true,None::<&str>)?;
    let exit=tauri::menu::MenuItem::with_id(app,"um-exit",crate::ui_locale::caption("tray_exit"),true,None::<&str>)?;
    tauri::menu::Menu::with_items(app,&[&open,&exit])
}
pub fn refresh_menu(app:&tauri::AppHandle) {if let Some(tray)=app.tray_by_id("um-tray") {if let Ok(menu)=menu(app){let _=tray.set_menu(Some(menu));}}}
pub fn setup(app:&tauri::AppHandle)->tauri::Result<()> {
    let mut builder=tauri::tray::TrayIconBuilder::with_id("um-tray").tooltip("Unreal Manager").menu(&menu(app)?).show_menu_on_left_click(false);
    if let Some(icon)=app.default_window_icon(){builder=builder.icon(icon.clone());}
    builder.on_menu_event(|app,event|match event.id.as_ref(){
        "um-open"=>show(app),
        "um-exit"=>{let handle=app.clone();tauri::async_runtime::spawn(async move{if active(&handle){show(&handle);let _=handle.emit("um-close-request","__closing__");}if let Err(error)=window_close_action(handle.clone(),"exit".into(),false).await{show(&handle);let _=handle.emit("um-close-request",error);}});},
        _=>{}
    }).on_tray_icon_event(|tray,event|{
        if matches!(event,tauri::tray::TrayIconEvent::Click{button:tauri::tray::MouseButton::Left,button_state:tauri::tray::MouseButtonState::Up,..}){show(tray.app_handle());}
    }).build(app)?;
    Ok(())
}
#[cfg(test)] mod tests {use super::*;#[test]fn validates_close_behavior(){assert!(valid("ask"));assert!(valid("tray"));assert!(valid("exit"));assert!(!valid("delete"));assert!(close_behavior_set("invalid".into()).is_err());}}

#[cfg(test)]mod closing_tests{
 use super::*;
 #[tokio::test]async fn closing_stops_a_paused_engine_and_waits_for_safe_completion(){
  let engine=std::sync::Arc::new(crate::engine::EngineState::default());let vault=std::sync::Arc::new(crate::vault::VaultState::default());
  engine.active.store(true,Ordering::SeqCst);engine.paused.store(true,Ordering::SeqCst);vault.active.store(true,Ordering::SeqCst);
  let e=engine.clone();let v=vault.clone();let task=tokio::spawn(async move{stop_operations(&e,&v).await;});
  tokio::time::sleep(std::time::Duration::from_millis(30)).await;
  assert!(engine.suspend.load(Ordering::SeqCst));assert!(engine.cancel.load(Ordering::SeqCst));assert!(!engine.paused.load(Ordering::SeqCst));assert!(vault.cancel.load(Ordering::SeqCst));assert!(!task.is_finished());
  engine.active.store(false,Ordering::SeqCst);vault.active.store(false,Ordering::SeqCst);
  tokio::time::timeout(std::time::Duration::from_secs(1),task).await.unwrap().unwrap();
 }
}

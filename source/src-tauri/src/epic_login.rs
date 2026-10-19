use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri::webview::PageLoadEvent;

const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";
const LABEL: &str = "epic-login";
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct ActiveGuard;
impl Drop for ActiveGuard { fn drop(&mut self) { ACTIVE.store(false, Ordering::SeqCst); } }

fn is_result_page(url: &tauri::Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("www.epicgames.com")
        && url.port_or_known_default() == Some(443) && url.path() == "/id/api/redirect"
        && url.query_pairs().any(|(k,v)| k == "clientId" && v == "34a02cf8f4414e29b15921876da36f9a")
}

fn callback_result(url: &tauri::Url, nonce: &str) -> Option<Result<String, String>> {
    if url.scheme() != "https" || url.host_str() != Some("localhost") || url.port_or_known_default() != Some(443)
        || url.path() != format!("/um-login/{nonce}") { return None; }
    Some(match url.query_pairs().find(|(k,_)| k == "code") {
        Some((_,code)) => super::epic::parse_code(&code),
        None => Err("Epic не вернул код входа. Закройте окно и попробуйте снова".into()),
    })
}

pub async fn authorize(app: &tauri::AppHandle, remember: bool) -> Result<String, String> {
    let _update_operation=crate::app_updates::operation()?;
    if ACTIVE.swap(true, Ordering::SeqCst) { return Err("Окно входа уже открыто".into()); }
    let _guard = ActiveGuard;
    super::epic_browser::clear_session(app)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let callback = format!("https://localhost/um-login/{nonce}");
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let navigation_sender = sender.clone();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(LOGIN_URL.parse().unwrap()))
        .title(crate::ui_locale::caption("login"))
        .inner_size(560.0, 760.0).min_inner_size(480.0, 600.0).center()
        .data_directory(super::epic_browser::profile(app)?)
        .incognito(!remember)
        .on_navigation(move |url| {
            // Intercept the one-time local callback before any network request.
            if let Some(result) = callback_result(url, &nonce) {
                if let Ok(mut pending) = navigation_sender.lock() { if let Some(tx) = pending.take() { let _ = tx.send(result); } }
                return false;
            }
            url.scheme() == "https" && url.host_str() != Some("localhost")
                && url.host_str() != Some("tauri.localhost")
                && !url.host_str().is_some_and(|h| h.parse::<std::net::IpAddr>().is_ok())
        })
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished || !is_result_page(payload.url()) { return; }
            // Read only Epic's final authorization response, never form fields or credentials.
            // No native IPC is enabled for this remote window.
            let target = serde_json::to_string(&callback).unwrap();
            let script = format!(r#"(() => {{
                if (location.origin !== 'https://www.epicgames.com' || location.pathname !== '/id/api/redirect') return;
                let code = '';
                try {{ code = JSON.parse(document.body.innerText).authorizationCode || ''; }} catch (_) {{}}
                document.body.textContent = 'Завершаем вход в Unreal Manager…';
                location.replace({target} + (code ? '?code=' + encodeURIComponent(code) : '?error=missing-code'));
            }})()"#);
            let _ = window.eval(&script);
        })
        .build().map_err(|_| "Не удалось открыть окно Epic Games")?;
    if crate::app_updates::applying() {
        let _=window.destroy();
        return Err("Вход отменён перед обновлением программы.".into());
    }
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            if let Ok(mut pending) = sender.lock() { if let Some(tx) = pending.take() { let _ = tx.send(Err("Вход отменён".into())); } }
        }
    });
    let result = match tokio::time::timeout(Duration::from_secs(600), receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("Вход отменён".into()),
        Err(_) => Err("Время ожидания входа истекло. Попробуйте снова".into()),
    };
    let _ = window.destroy();
    super::epic_browser::restore_main(app);
    result
}

#[tauri::command]
pub fn epic_cancel_login(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) { window.destroy().map_err(|_| "Не удалось закрыть окно входа")?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_origin_is_exact() {
        let base = "https://www.epicgames.com/id/api/redirect?clientId=34a02cf8f4414e29b15921876da36f9a";
        assert!(is_result_page(&base.parse().unwrap()));
        for bad in [base.replace("https:", "http:"), base.replace(".com/", ".com.evil/"), base.replace("/redirect?", "/other?"), base.replace("clientId=34", "clientId=99")] {
            assert!(!is_result_page(&bad.parse().unwrap()));
        }
    }
    #[test]
    fn callback_requires_current_nonce_and_valid_code() {
        assert!(callback_result(&"https://localhost/um-login/current?code=syntheticCode12345".parse().unwrap(), "current").unwrap().is_ok());
        assert!(callback_result(&"https://localhost/um-login/old?code=syntheticCode12345".parse().unwrap(), "current").is_none());
        assert!(callback_result(&"https://localhost.evil/um-login/current?code=syntheticCode12345".parse().unwrap(), "current").is_none());
        assert!(callback_result(&"https://localhost/um-login/current?code=bad".parse().unwrap(), "current").unwrap().is_err());
    }
}

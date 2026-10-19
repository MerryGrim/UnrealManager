use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub fn profile(_app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(crate::storage::root().join("Account/epic-web"))
}

pub fn restore_main(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(main) = handle.get_webview_window("main") {
            let _ = main.show();
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
    });
}

// Only the dedicated Epic profile is cleared; the app's library/history is separate.
pub fn clear_session(app: &tauri::AppHandle) -> Result<(), String> {
    for label in ["epic-login", "epic-redeem"] {
        if app.get_webview_window(label).is_some() {
            return Err("Сначала закройте окно Epic Games".into());
        }
    }
    let window = WebviewWindowBuilder::new(app, "epic-session-reset", WebviewUrl::External("about:blank".parse().unwrap()))
        .data_directory(profile(app)?).visible(false).build().map_err(|_| "Не удалось очистить веб-сеанс Epic")?;
    let result = (|| {
        let cookies = window.cookies().map_err(|_| "Не удалось прочитать веб-сеанс Epic".to_string())?;
        for cookie in cookies {
            window.delete_cookie(cookie).map_err(|_| "Не удалось завершить веб-сеанс Epic".to_string())?;
        }
        Ok(())
    })();
    let _ = window.destroy();
    result
}

use std::sync::atomic::{AtomicUsize,Ordering};
static LANGUAGE:AtomicUsize=AtomicUsize::new(1);
const LANGUAGES:[&str;9]=["ru","en","de","it","es","fr","zh","ja","ko"];
pub(crate) fn language()->&'static str {
    LANGUAGES.get(LANGUAGE.load(Ordering::Relaxed)).copied().unwrap_or("en")
}
#[tauri::command]
pub fn set_ui_language(app:tauri::AppHandle,language:String)->Result<(),String>{
    let index=LANGUAGES.iter().position(|x|*x==language).ok_or("Unsupported language")?;
    LANGUAGE.store(index,Ordering::Relaxed);crate::window_behavior::refresh_menu(&app);Ok(())
}
pub fn caption(key:&str)->&'static str {
    let values=match key {
        "tray_open"=>["Открыть Unreal Manager","Open Unreal Manager","Unreal Manager öffnen","Apri Unreal Manager","Abrir Unreal Manager","Ouvrir Unreal Manager","打开 Unreal Manager","Unreal Manager を開く","Unreal Manager 열기"],
        "tray_exit"=>["Выход","Exit","Beenden","Esci","Salir","Quitter","退出","終了","종료"],
        "login"=>["Вход в Epic Games — www.epicgames.com","Sign in to Epic Games — www.epicgames.com","Bei Epic Games anmelden — www.epicgames.com","Accedi a Epic Games — www.epicgames.com","Iniciar sesión en Epic Games — www.epicgames.com","Se connecter à Epic Games — www.epicgames.com","登录 Epic Games — www.epicgames.com","Epic Games にサインイン — www.epicgames.com","Epic Games 로그인 — www.epicgames.com"],
        "redeem"=>["Активация ключа — официальный сайт Epic Games","Redeem a key — official Epic Games website","Schlüssel aktivieren — offizielle Epic-Games-Website","Attiva un codice — sito ufficiale Epic Games","Canjear código — sitio oficial de Epic Games","Activer un code — site officiel Epic Games","激活兑换码 — Epic Games 官方网站","キーを引き換える — Epic Games 公式サイト","키 등록 — Epic Games 공식 웹사이트"],
        "folder"=>["Выберите папку","Select a folder","Ordner auswählen","Seleziona una cartella","Seleccionar carpeta","Choisir un dossier","选择文件夹","フォルダーを選択","폴더 선택"],
        "project"=>["Выберите проект Unreal Engine","Select an Unreal Engine project","Unreal-Engine-Projekt auswählen","Seleziona un progetto Unreal Engine","Seleccionar proyecto de Unreal Engine","Choisir un projet Unreal Engine","选择 Unreal Engine 项目","Unreal Engine プロジェクトを選択","Unreal Engine 프로젝트 선택"],
        _=>["Проекты Unreal Engine (*.uproject)|*.uproject","Unreal Engine projects (*.uproject)|*.uproject","Unreal-Engine-Projekte (*.uproject)|*.uproject","Progetti Unreal Engine (*.uproject)|*.uproject","Proyectos Unreal Engine (*.uproject)|*.uproject","Projets Unreal Engine (*.uproject)|*.uproject","Unreal Engine 项目 (*.uproject)|*.uproject","Unreal Engine プロジェクト (*.uproject)|*.uproject","Unreal Engine 프로젝트 (*.uproject)|*.uproject"],
    };
    values[LANGUAGE.load(Ordering::Relaxed)]
}


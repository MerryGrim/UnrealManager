#ifndef AppVersion
  #define AppVersion "0.9.4"
#endif
#ifndef OutputRoot
  #define OutputRoot SourcePath + "..\..\Releases\" + AppVersion
#endif
#ifndef AppExecutable
  #define AppExecutable SourcePath + "..\src-tauri\target\release\unreal-manager.exe"
#endif

[Setup]
AppId={{61185C58-31CF-4783-BA27-429C3454B9A5}
AppName=Unreal Manager
AppVersion={#AppVersion}
AppPublisher=Nyanma
AppPublisherURL=https://github.com/MerryGrim
DefaultDirName={localappdata}\Programs\Unreal Manager
DefaultGroupName=Unreal Manager
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
ShowLanguageDialog=yes
LanguageDetectionMethod=uilanguage
OutputDir={#OutputRoot}
OutputBaseFilename=UnrealManager-{#AppVersion}-Windows-x64-Setup
SetupIconFile=..\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\unreal-manager.exe
Compression=lzma2
SolidCompression=yes
CloseApplications=no
RestartApplications=no
DisableProgramGroupPage=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "zh"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"

[CustomMessages]
en.RuntimeFailed=Microsoft WebView2 could not be installed. Check your Internet connection and try again.
ru.RuntimeFailed=Не удалось установить Microsoft WebView2. Проверьте подключение к Интернету и повторите попытку.
de.RuntimeFailed=Microsoft WebView2 konnte nicht installiert werden. Prüfen Sie Ihre Internetverbindung und versuchen Sie es erneut.
it.RuntimeFailed=Impossibile installare Microsoft WebView2. Verifica la connessione Internet e riprova.
es.RuntimeFailed=No se pudo instalar Microsoft WebView2. Compruebe la conexión a Internet e inténtelo de nuevo.
fr.RuntimeFailed=Impossible d’installer Microsoft WebView2. Vérifiez votre connexion Internet et réessayez.
zh.RuntimeFailed=无法安装 Microsoft WebView2。请检查网络连接并重试。
ja.RuntimeFailed=Microsoft WebView2 をインストールできませんでした。インターネット接続を確認して再試行してください。
ko.RuntimeFailed=Microsoft WebView2를 설치하지 못했습니다. 인터넷 연결을 확인하고 다시 시도하세요.
en.UpdateRepairFailed=The interrupted application update was not repaired. Setup stopped before copying application files. Keep the update journal and backups; use a trusted new portable copy for recovery.
ru.UpdateRepairFailed=Прерванное обновление приложения не восстановлено. Установка остановлена до копирования файлов приложения. Сохраните журнал и резервные копии; для восстановления используйте новую доверенную portable-копию.
de.UpdateRepairFailed=Das unterbrochene App-Update wurde nicht repariert. Setup wurde vor dem Kopieren der App-Dateien angehalten. Bewahren Sie Journal und Sicherungen auf; verwenden Sie zur Wiederherstellung eine neue vertrauenswürdige portable Kopie.
it.UpdateRepairFailed=L'aggiornamento interrotto non è stato riparato. L'installazione si è fermata prima di copiare i file dell'app. Conserva il registro e le copie di sicurezza; usa una nuova copia portable attendibile per il ripristino.
es.UpdateRepairFailed=No se reparó la actualización interrumpida. La instalación se detuvo antes de copiar los archivos de la aplicación. Conserve el registro y las copias de seguridad; use una nueva copia portable de confianza para recuperar la aplicación.
fr.UpdateRepairFailed=La mise à jour interrompue n'a pas été réparée. L'installation s'est arrêtée avant de copier les fichiers de l'application. Conservez le journal et les sauvegardes ; utilisez une nouvelle copie portable fiable pour la restauration.
zh.UpdateRepairFailed=中断的应用更新未修复。安装程序已在复制应用文件前停止。请保留更新日志和备份，并使用可信的新便携版进行恢复。
ja.UpdateRepairFailed=中断された更新は修復されませんでした。アプリのファイルをコピーする前にインストールを停止しました。更新記録とバックアップを保持し、信頼できる新しいポータブル版で復旧してください。
ko.UpdateRepairFailed=중단된 앱 업데이트를 복구하지 못했습니다. 앱 파일을 복사하기 전에 설치가 중지되었습니다. 업데이트 기록과 백업을 보관하고 신뢰할 수 있는 새 포터블 버전으로 복구하세요.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "dependencies\MicrosoftEdgeWebview2Setup.exe"; Flags: dontcopy
Source: "{#AppExecutable}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "THIRD-PARTY-LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Unreal Manager"; Filename: "{app}\unreal-manager.exe"
Name: "{autodesktop}\Unreal Manager"; Filename: "{app}\unreal-manager.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\unreal-manager.exe"; Description: "{cm:LaunchProgram,Unreal Manager}"; Flags: nowait postinstall skipifsilent

[Code]
function RuntimeIn(RootKey: Integer): Boolean;
var Version: String;
begin
  Result := RegQueryStringValue(RootKey,
    'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Version)
    and (Version <> '') and (Version <> '0.0.0.0');
end;

function HasWebView: Boolean;
begin
  Result := RuntimeIn(HKLM32) or RuntimeIn(HKCU32) or RuntimeIn(HKLM64) or RuntimeIn(HKCU64);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var ResultCode: Integer; JournalPath: String;
begin
  Result := '';
  JournalPath := ExpandConstant('{app}\.um-app-update.json');
  if FileExists(JournalPath) then begin
    { This is the trusted incoming EXE, not a possibly damaged installed EXE.
      Its headless CLI validates the signed journal and either recognizes the
      exact live updater installer or asks before retaining/restoring files. }
    ExtractTemporaryFile('unreal-manager.exe');
    if not Exec(ExpandConstant('{tmp}\unreal-manager.exe'),
      '--repair-app-update "' + JournalPath + '" --no-restart --installer "' +
      ExpandConstant('{srcexe}') + '"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
      Result := CustomMessage('UpdateRepairFailed')
    else if ResultCode <> 0 then
      Result := CustomMessage('UpdateRepairFailed');
    if Result <> '' then exit;
  end;
  if not HasWebView then begin
    ExtractTemporaryFile('MicrosoftEdgeWebview2Setup.exe');
    if not Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'),
      '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
      Result := CustomMessage('RuntimeFailed')
    else if not HasWebView then
      Result := CustomMessage('RuntimeFailed');
  end;
end;

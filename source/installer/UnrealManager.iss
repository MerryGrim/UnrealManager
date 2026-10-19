#ifndef AppVersion
  #define AppVersion "0.2.0-beta.6"
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

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "dependencies\MicrosoftEdgeWebview2Setup.exe"; Flags: dontcopy
Source: "{#AppExecutable}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

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
var ResultCode: Integer;
begin
  Result := '';
  if not HasWebView then begin
    ExtractTemporaryFile('MicrosoftEdgeWebview2Setup.exe');
    if not Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'),
      '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
      Result := CustomMessage('RuntimeFailed')
    else if not HasWebView then
      Result := CustomMessage('RuntimeFailed');
  end;
end;

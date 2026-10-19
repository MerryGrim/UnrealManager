use serde::{Deserialize, Serialize};
use std::{sync::{LazyLock, Mutex}, time::{Duration, SystemTime, UNIX_EPOCH}};

const DAY: u64 = 86400;
static CHECK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static OPTIONS: LazyLock<Mutex<Options>> = LazyLock::new(|| Mutex::new(
    std::fs::read(crate::storage::settings().join("app-updates.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()));

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", default)]
struct Options { enabled: bool, channel: String, last_attempt: u64 }
impl Default for Options { fn default()->Self { Self {enabled:true, channel:"stable".into(),last_attempt:0} } }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Settings { enabled: bool, channel: String, configured: bool }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Release { version: String, notes: String, url: String, download_url: String, size: u64 }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct CheckResult { status: String, release: Option<Release>, checked_at: u64, cached: bool }
#[derive(Serialize, Deserialize)]
struct Cached { repository: String, channel: String, current: String, result: CheckResult }
#[derive(Deserialize)]
struct GithubRelease { tag_name: String, draft: bool, prerelease: bool, #[serde(default)] body: Option<String>, assets: Vec<Asset> }
#[derive(Deserialize)]
struct Asset { name: String, browser_download_url: String, size: u64, state: String }

fn repository()->Option<&'static str> { option_env!("UM_RELEASE_REPOSITORY").filter(|v|valid_repository(v)) }
fn valid_repository(v:&str)->bool {
    let Some((owner,name))=v.split_once('/') else{return false};
    owner=="MerryGrim" && !name.is_empty() && name!="." && name!=".." && name.len()<=100 && name.bytes().all(|c|c.is_ascii_alphanumeric()||b"-_.".contains(&c))
}
fn now()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}
fn persist(options:&Options)->Result<(),String> {crate::storage::atomic(&crate::storage::settings().join("app-updates.json"),&serde_json::to_vec(options).map_err(|e|e.to_string())?)}
#[tauri::command]
pub fn app_update_settings_get()->Settings {let p=OPTIONS.lock().unwrap_or_else(|e|e.into_inner());Settings{enabled:p.enabled,channel:p.channel.clone(),configured:repository().is_some()}}
#[tauri::command]
pub fn app_update_settings_set(enabled:bool,channel:String)->Result<(),String> {
    if !matches!(channel.as_str(),"stable"|"beta"){return Err("Invalid update channel".into());}
    let mut p=OPTIONS.lock().map_err(|e|e.to_string())?;
    let next=Options{enabled,channel:channel.clone(),last_attempt:if channel==p.channel{p.last_attempt}else{0}};
    persist(&next)?;*p=next;Ok(())
}
fn cached(repo:&str,channel:&str,current:&str)->Option<CheckResult> {
    let c:Cached=serde_json::from_slice(&std::fs::read(crate::storage::cache().join("app-updates.json")).ok()?).ok()?;
    if c.repository!=repo || c.channel!=channel || c.current!=current {return None;}
    let mut result=c.result;result.cached=true;Some(result)
}
fn empty(status:&str)->CheckResult {CheckResult{status:status.into(),release:None,checked_at:0,cached:false}}
fn release_url(repo:&str,tag:&str)->String {
    let mut url=reqwest::Url::parse(&format!("https://github.com/{repo}/releases/tag/")).unwrap();
    url.path_segments_mut().unwrap().pop_if_empty().push(tag);url.into()
}
fn trusted_asset(url:&str,repo:&str,tag:&str,name:&str)->bool {
    let mut expected=reqwest::Url::parse(&format!("https://github.com/{repo}/releases/download/")).unwrap();
    expected.path_segments_mut().unwrap().pop_if_empty().push(tag).push(name);
    url==expected.as_str()
}
fn select(releases:Vec<GithubRelease>,repo:&str,current:&str,channel:&str)->Result<Option<Release>,String> {
    let current=semver::Version::parse(current).map_err(|_|"Invalid application version")?;
    let mut selected:Option<(semver::Version,Release)>=None;
    for r in releases {
        let Ok(version)=semver::Version::parse(r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name)) else{continue};
        if r.draft || (channel=="stable" && (r.prerelease||!version.pre.is_empty())) || version<=current {continue;}
        let name=format!("UnrealManager-{version}-Windows-x64-Setup.exe");
        let Some(asset)=r.assets.into_iter().find(|a|a.name==name&&a.state=="uploaded"&&a.size>0&&trusted_asset(&a.browser_download_url,repo,&r.tag_name,&a.name)) else{continue};
        if selected.as_ref().is_some_and(|(v,_)|v>=&version){continue;}
        let release=Release{version:version.to_string(),notes:r.body.unwrap_or_default().chars().take(12000).collect(),url:release_url(repo,&r.tag_name),download_url:asset.browser_download_url,size:asset.size};
        selected=Some((version,release));
    }
    Ok(selected.map(|(_,r)|r))
}
#[tauri::command]
pub async fn app_update_check(manual:bool)->Result<CheckResult,String> {
    let _guard=CHECK.try_lock().map_err(|_|"Проверка обновлений уже выполняется.")?;
    let Some(repo)=repository() else{return Ok(empty("unconfigured"))};
    let options=OPTIONS.lock().map_err(|e|e.to_string())?.clone();
    if !manual&&!options.enabled{return Ok(empty("disabled"));}
    let current=env!("CARGO_PKG_VERSION");
    let timestamp=now();
    if !manual&&timestamp.saturating_sub(options.last_attempt)<DAY {
        return Ok(cached(repo,&options.channel,current).unwrap_or_else(||empty("deferred")));
    }
    {let mut p=OPTIONS.lock().map_err(|e|e.to_string())?;p.last_attempt=timestamp;persist(&p)?;}
    let client=reqwest::Client::builder().user_agent(format!("UnrealManager/{current}"))
        .timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
    let response=client.get(format!("https://api.github.com/repos/{repo}/releases?per_page=100"))
        .header("Accept","application/vnd.github+json").header("X-GitHub-Api-Version","2022-11-28")
        .send().await.map_err(|_|"Не удалось проверить обновления. Проверьте подключение к интернету.")?;
    if !response.status().is_success(){return Err("GitHub временно недоступен или релизы ещё не опубликованы.".into());}
    let mut response=response;let mut data=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|"Не удалось прочитать ответ GitHub.")? {
        if data.len()+chunk.len()>2*1024*1024{return Err("Ответ GitHub слишком большой.".into());}data.extend_from_slice(&chunk);
    }
    let releases=serde_json::from_slice(&data).map_err(|_|"Некорректный список релизов GitHub.")?;
    let release=select(releases,repo,current,&options.channel)?;
    let result=CheckResult{status:if release.is_some(){"available"}else{"current"}.into(),release,checked_at:timestamp,cached:false};
    // A channel changed during the request must not leak an update from the old channel.
    if OPTIONS.lock().map_err(|e|e.to_string())?.channel!=options.channel{return Ok(empty("deferred"));}
    let saved=Cached{repository:repo.into(),channel:options.channel,current:current.into(),result:result.clone()};
    crate::storage::atomic(&crate::storage::cache().join("app-updates.json"),&serde_json::to_vec(&saved).map_err(|e|e.to_string())?)?;
    Ok(result)
}
#[tauri::command]
pub fn app_update_open(version:String,download:bool)->Result<(),String> {
    let repo=repository().ok_or("Источник обновлений ещё не настроен.")?;
    let channel=OPTIONS.lock().map_err(|e|e.to_string())?.channel.clone();
    let result=cached(repo,&channel,env!("CARGO_PKG_VERSION")).ok_or("Сначала проверьте обновления.")?;
    let release=result.release.filter(|r|r.version==version).ok_or("Сначала проверьте обновления.")?;
    let expected_name=format!("UnrealManager-{}-Windows-x64-Setup.exe",release.version);
    let parsed=semver::Version::parse(&release.version).map_err(|_|"Invalid release version")?;
    if parsed<=semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap(){return Err("Invalid release version".into());}
    // Validate cached URLs as well: cache files are not a source of trusted executable links.
    let plain=release_url(repo,&release.version);let prefixed=release_url(repo,&format!("v{}",release.version));
    let tag=if release.url==plain {release.version.clone()}else if release.url==prefixed {format!("v{}",release.version)}else{return Err("Некорректная ссылка обновления.".into())};
    if !trusted_asset(&release.download_url,repo,&tag,&expected_name){return Err("Некорректная ссылка обновления.".into());}
    webbrowser::open(if download{&release.download_url}else{&release.url}).map_err(|e|e.to_string())
}
#[cfg(test)]mod tests {
 use super::*;
 fn release(version:&str,prerelease:bool)->GithubRelease {let tag=format!("v{version}");let name=format!("UnrealManager-{version}-Windows-x64-Setup.exe");GithubRelease{tag_name:tag.clone(),draft:false,prerelease,body:Some("Notes".into()),assets:vec![Asset{name:name.clone(),browser_download_url:format!("https://github.com/MerryGrim/UM/releases/download/{tag}/{name}"),size:123,state:"uploaded".into()}]}}
 #[test]fn stable_and_beta_use_semantic_order_and_require_installers(){
  let versions=||vec![release("0.2.0-beta.10",true),release("0.2.0-beta.6",true),release("0.2.0",false),release("0.3.0-beta.1",true)];
  assert_eq!(select(versions(),"MerryGrim/UM","0.2.0-beta.5","stable").unwrap().unwrap().version,"0.2.0");
  assert_eq!(select(versions(),"MerryGrim/UM","0.2.0-beta.5","beta").unwrap().unwrap().version,"0.3.0-beta.1");
  assert_eq!(select(vec![release("0.2.0-beta.9",true),release("0.2.0-beta.10",true)],"MerryGrim/UM","0.2.0-beta.5","beta").unwrap().unwrap().version,"0.2.0-beta.10");
  assert!(select(vec![release("0.2.0",false)],"MerryGrim/UM","0.2.0","beta").unwrap().is_none());
 }
 #[test]fn refuses_drafts_missing_installers_and_foreign_links(){
  let mut draft=release("1.0.0",false);draft.draft=true;
  let mut missing=release("1.1.0",false);missing.assets.clear();
  let mut foreign=release("1.2.0",false);foreign.assets[0].browser_download_url="https://example.com/setup.exe".into();
  assert!(select(vec![draft,missing,foreign],"MerryGrim/UM","0.2.0","beta").unwrap().is_none());
  assert!(!valid_repository("MerryGrim/../other"));assert!(!valid_repository("someone/UM"));assert!(valid_repository("MerryGrim/UM"));
 }
}

//! Legacy owned listings whose current Fab metadata route is absent.
//! Fetch manifest descriptions only; never request or retain asset chunks.
use egs_api::api::types::fab_asset_manifest::FabAssetManifest;
use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
use std::time::Duration;

const INFO_LIMIT:usize=1024*1024;
const MANIFEST_LIMIT:usize=8*1024*1024;
const DEADLINE:Duration=Duration::from_secs(30);
const SDK_AGENT:&str="UELauncher/17.0.1-37584233+++Portal+Release-Live Windows/10.0.19043.1.0.64bit";
const CDN_HOSTS:&[&str]=&[
    "download.epicgames.com","download2.epicgames.com","download3.epicgames.com","download4.epicgames.com",
    "cdn1.unrealengine.com","epicgames-download1.akamaized.net","egdownload.fastly-edge.com",
];

#[cfg(feature="engine-test-cli")]
static REJECTED_HOSTS:std::sync::Mutex<std::collections::BTreeSet<String>>=std::sync::Mutex::new(std::collections::BTreeSet::new());
#[cfg(feature="engine-test-cli")]
pub(super) fn rejected_hosts()->Vec<String>{REJECTED_HOSTS.lock().map(|hosts|hosts.iter().cloned().collect()).unwrap_or_default()}
#[cfg(feature="engine-test-cli")]
static REJECTED_FORMATS:std::sync::Mutex<std::collections::BTreeSet<String>>=std::sync::Mutex::new(std::collections::BTreeSet::new());
#[cfg(feature="engine-test-cli")]
pub(super) fn rejected_formats()->Vec<String>{REJECTED_FORMATS.lock().map(|formats|formats.iter().cloned().collect()).unwrap_or_default()}
#[cfg(feature="engine-test-cli")]
static STAGES:std::sync::Mutex<std::collections::BTreeMap<String,usize>>=std::sync::Mutex::new(std::collections::BTreeMap::new());
#[cfg(feature="engine-test-cli")]
static STATUSES:std::sync::Mutex<std::collections::BTreeMap<String,usize>>=std::sync::Mutex::new(std::collections::BTreeMap::new());
#[cfg(feature="engine-test-cli")]
pub(super) fn diagnostics()->serde_json::Value{serde_json::json!({"stages":STAGES.lock().map(|values|values.clone()).unwrap_or_default(),"httpStatuses":STATUSES.lock().map(|values|values.clone()).unwrap_or_default()})}
fn note(stage:&'static str){
    #[cfg(feature="engine-test-cli")]
    if let Ok(mut values)=STAGES.lock(){*values.entry(stage.into()).or_default()+=1;}
    #[cfg(not(feature="engine-test-cli"))]
    let _=stage;
}
fn status(stage:&'static str,code:u16){
    #[cfg(feature="engine-test-cli")]
    if let Ok(mut values)=STATUSES.lock(){*values.entry(format!("{stage}:{code}")).or_default()+=1;}
    #[cfg(not(feature="engine-test-cli"))]
    let _=(stage,code);
}

fn identifier(value:&str)->bool {
    !value.is_empty()&&value.len()<=256&&!matches!(value,"."|"..")&&value.bytes().all(|b|b.is_ascii_alphanumeric()||b"_- .".contains(&b))&&!value.contains(' ')
}
fn unreal_format(value:&str)->bool {
    // Captured Fab artifact format taxonomy; distinct from listing distribution enums.
    if value.eq_ignore_ascii_case("asset-format/game-engine/unreal-engine"){return true;}
    value.len()<=64&&value.trim().to_ascii_lowercase().chars().filter(|c|!matches!(c,'_'|'-'|' ')).collect::<String>()=="unrealengine"
}
fn cdn_url(value:&str)->Option<reqwest::Url> {
    if value.len()>8192{return None;}
    let url=reqwest::Url::parse(value).ok()?;
    if url.scheme()!="https"||url.port_or_known_default()!=Some(443)||!url.username().is_empty()||url.password().is_some()||url.fragment().is_some(){return None;}
    let host=url.host_str()?;
    if !CDN_HOSTS.contains(&host){
        #[cfg(feature="engine-test-cli")]
        if host.len()<=253&&host.bytes().all(|b|b.is_ascii_alphanumeric()||b".-".contains(&b)){if let Ok(mut hosts)=REJECTED_HOSTS.lock(){if hosts.len()<16{hosts.insert(host.into());}}}
        return None;
    }
    Some(url)
}
fn under_base(manifest:&reqwest::Url,base:&reqwest::Url)->bool {
    manifest.host_str()==base.host_str()&&manifest.port_or_known_default()==base.port_or_known_default()&&base.query().is_none()
        &&(manifest.path()==base.path()||manifest.path().starts_with(&format!("{}/",base.path().trim_end_matches('/'))))
}
fn manifest_url(info:&FabAssetManifest,artifact:&str,now:i64)->Option<reqwest::Url>{
    if info.download_info.is_empty()||info.download_info.len()>8{note("InfoItems");return None;}
    for item in &info.download_info {
        if item.artifact_id!=artifact{note("InfoIdentity");continue;}
        if !unreal_format(&item.asset_format){
            note("InfoFormat");
            #[cfg(feature="engine-test-cli")]
            {
                let observed=if item.asset_format.len()<=128&&item.asset_format.bytes().all(|b|b.is_ascii_alphanumeric()||b"_- .+/".contains(&b)){item.asset_format.clone()}else{
                    use sha2::{Digest,Sha256};
                    format!("<unsafe length={} sha256={:x}>",item.asset_format.len(),Sha256::digest(item.asset_format.as_bytes()))
                };
                if let Ok(mut formats)=REJECTED_FORMATS.lock(){if formats.len()<16{formats.insert(observed);}}
            }
            continue;
        }
        if item.distribution_points.is_empty()||item.distribution_points.len()>8||item.distribution_point_base_urls.is_empty()||item.distribution_point_base_urls.len()>8{note("InfoPoints");continue;}
        for point in &item.distribution_points {
            if point.signature_expiration.unix_timestamp()<=now+5{note("InfoExpired");continue;}
            let Some(url)=cdn_url(&point.manifest_url)else{note("InfoCdnRejected");continue;};
            if item.distribution_point_base_urls.iter().filter_map(|base|cdn_url(base)).any(|base|under_base(&url,&base)){return Some(url);}
            note("InfoBase");
        }
    }
    None
}
async fn bounded(mut response:reqwest::Response,limit:usize,budget:&AtomicUsize)->Option<Vec<u8>> {
    if !response.status().is_success()||response.content_length().is_some_and(|size|size>limit as u64){return None;}
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len())>limit||budget.fetch_add(chunk.len(),Ordering::Relaxed).saturating_add(chunk.len())>super::MAX_TRANSFER{return None;}
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

pub(super) async fn classify(client:&reqwest::Client,token:&str,namespace:&str,asset_id:&str,artifact:&str,budget:Arc<AtomicUsize>)->Option<String>{
    if token.is_empty()||![namespace,asset_id,artifact].into_iter().all(identifier){note("Input");return None;}
    let result=tokio::time::timeout(DEADLINE,async {
        let endpoint=format!("https://www.fab.com/e/artifacts/{artifact}/manifest");
        let correlation=format!("UE4-{}-{}-{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple().to_string().to_ascii_uppercase(),uuid::Uuid::new_v4().simple().to_string().to_ascii_uppercase());
        let response=match client.post(&endpoint).bearer_auth(token).header(reqwest::header::USER_AGENT,SDK_AGENT).header("X-Epic-Correlation-ID",correlation).json(&serde_json::json!({"item_id":asset_id,"namespace":namespace,"platform":"Windows"})).send().await{Ok(response)=>response,Err(_)=>{note("InfoRequest");return None;}};
        status("Info",response.status().as_u16());
        if response.url().as_str()!=endpoint{note("InfoRedirect");return None;}
        if !response.status().is_success(){note("InfoHttp");return None;}
        let bytes=match bounded(response,INFO_LIMIT,&budget).await{Some(bytes)=>bytes,None=>{note("InfoBody");return None;}};
        let info:FabAssetManifest=match serde_json::from_slice(&bytes){Ok(info)=>info,Err(_)=>{note("InfoJson");return None;}};
        let url=manifest_url(&info,artifact,chrono::Utc::now().timestamp())?;
        // A separate no-cookie/no-auth client prevents a caller's default
        // credentials from ever accompanying the signed CDN request.
        static CDN:std::sync::OnceLock<Option<reqwest::Client>>=std::sync::OnceLock::new();
        let cdn=CDN.get_or_init(||reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(DEADLINE).user_agent(concat!("UnrealManager/",env!("CARGO_PKG_VERSION"))).build().ok()).as_ref()?;
        let response=match cdn.get(url.clone()).send().await{Ok(response)=>response,Err(_)=>{note("ManifestRequest");return None;}};
        status("Manifest",response.status().as_u16());
        if response.url()!=&url{note("ManifestRedirect");return None;}
        if !response.status().is_success(){note("ManifestHttp");return None;}
        let bytes=match bounded(response,MANIFEST_LIMIT,&budget).await{Some(bytes)=>bytes,None=>{note("ManifestBody");return None;}};
        if bytes.iter().find(|byte|!byte.is_ascii_whitespace())==Some(&b'{'){note("JsonManifest");}else{note("BinaryManifest");}
        // Keep bounded parsing inside this owned task so abort/drain cannot leave a detached parser.
        let kind=match crate::engine::package_content_kind(&bytes){Ok(kind)=>kind,Err(_)=>{note("ManifestParse");return None;}};
        if matches!(kind.as_str(),"Asset"|"Project"|"Plugin"){note("Success");Some(kind)}else{note("ManifestKind");None}
    }).await;
    match result{Ok(kind)=>kind,Err(_)=>{note("Deadline");None}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn cdn_is_https_closed_and_path_bound(){
        for value in ["https://download.epicgames.com/path/file.manifest?sig=opaque","https://egdownload.fastly-edge.com/Builds/test.manifest"]{assert!(cdn_url(value).is_some());}
        for value in ["http://download.epicgames.com/test","https://download.epicgames.com.evil.invalid/test","https://arbitrary.cloudfront.net/test","https://user:pass@download.epicgames.com/test","https://download.epicgames.com:444/test","https://127.0.0.1/test"]{assert!(cdn_url(value).is_none());}
        let manifest=cdn_url("https://download.epicgames.com/Builds/Project/file.manifest?sig=opaque").unwrap();
        assert!(under_base(&manifest,&cdn_url("https://download.epicgames.com/Builds/Project/").unwrap()));
        assert!(!under_base(&manifest,&cdn_url("https://download.epicgames.com/Builds/Proj").unwrap()));
    }
    fn info(artifact:&str,format:&str,expiration:&str)->FabAssetManifest{
        serde_json::from_value(serde_json::json!({"downloadInfo":[{"artifactId":artifact,"assetFormat":format,"buildVersion":"fixture","distributionPointBaseUrls":["https://download.epicgames.com/Builds/Project/"],"distributionPoints":[{"manifestUrl":"https://download.epicgames.com/Builds/Project/file.manifest?sig=opaque","signatureExpiration":expiration}],"manifestHash":"fixture","metadata":{},"type":"fixture"}]})).unwrap()
    }
    #[test]fn download_info_requires_identity_unreal_format_and_future_signature(){
        let now=chrono::DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z").unwrap().timestamp();
        assert!(manifest_url(&info("owned-artifact","unreal-engine","2026-10-26T11:00:00Z"),"owned-artifact",now).is_some());
        assert!(manifest_url(&info("owned-artifact","asset-format/game-engine/unreal-engine","2026-10-26T11:00:00Z"),"owned-artifact",now).is_some());
        assert!(manifest_url(&info("other-artifact","unreal-engine","2026-10-26T11:00:00Z"),"owned-artifact",now).is_none());
        assert!(manifest_url(&info("owned-artifact","fbx","2026-10-26T11:00:00Z"),"owned-artifact",now).is_none());
        assert!(manifest_url(&info("owned-artifact","unreal-engine","2026-10-26T09:00:00Z"),"owned-artifact",now).is_none());
    }
}

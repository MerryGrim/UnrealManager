//! A bounded, explicit-confirmation redemption session. No key is serialized in
//! snapshots, saved to storage, passed in a URL, or handed to the fallback page.
use std::{future::Future, pin::Pin, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::Duration};
use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::sync::oneshot;

const INVALID: &str = "Некорректный список ключей.";
const BUSY: &str = "Проверка ключей уже выполняется.";
const STALE: &str = "Подтверждение ключей устарело. Проверьте ключи заново.";
const RUNNING: &str = "Сначала остановите текущую активацию.";
const LABEL: &str = "epic-redeem-batch";
const PREVIEW_SECONDS: u64 = 300;
const SESSION_SECONDS: u64 = 600;
const INTER_REQUEST_DELAY: Duration = Duration::from_secs(2);
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn is_open() -> bool { ACTIVE.load(Ordering::SeqCst) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
enum Reason { InvalidInput, Duplicate, NotFound, AlreadyUsed, AlreadyOwned, Expired, RegionRestricted, AuthRequired, AccountChanged, PreviewExpired, RequiresOfficial, Network, Service, RateLimited, Unknown, Cancelled }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all="camelCase")]
enum Status { Pending, Checking, Ready, Activating, Success, Failed, Unknown, NotAttempted, RequiresOfficial }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all="camelCase")]
enum Phase { Preparing, Ready, Activating, Completed, Stopped }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all="camelCase")]
enum WaitReason { Pacing, RateLimited, Batch, BeforeActivation }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all="camelCase")]
enum PreparationStage { Account, Session, Connection, Keys }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
struct Item { id: String, index: usize, masked_key: String, status: Status, title: String, reason: Option<Reason>, #[serde(skip_serializing_if="Option::is_none")] diagnostic: Option<Diagnostic> }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Snapshot { batch_id: String, account_name: String, phase: Phase, items: Vec<Item>, #[serde(skip_serializing_if="Option::is_none")] expires_at: Option<u64>, #[serde(skip_serializing_if="Option::is_none")] wait_seconds: Option<u32>, #[serde(skip_serializing_if="Option::is_none")] wait_reason: Option<WaitReason>, #[serde(skip_serializing_if="Option::is_none")] preparation_stage:Option<PreparationStage> }
struct Secret(Vec<u8>);
impl Secret { fn text(&self) -> &str { std::str::from_utf8(&self.0).expect("validated ASCII key") } }
impl Drop for Secret { fn drop(&mut self) { self.0.fill(0); } }
#[derive(Clone, Deserialize)]
#[serde(default, rename_all="camelCase", deny_unknown_fields)]
struct Product { namespace: String, offer_id: String, entitlement_name: String, entitlement_name_is_null: bool, title: String, eula_ids: Vec<String>, code_use_id: String }
impl Default for Product { fn default() -> Self { Self { namespace:String::new(),offer_id:String::new(),entitlement_name:String::new(),entitlement_name_is_null:false,title:String::new(),eula_ids:Vec::new(),code_use_id:String::new() } } }
#[derive(Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Reply { id: String, ok: bool, reason: Option<Reason>, product: Option<Product>, account_id: Option<String>, account_name: Option<String>, diagnostic: Option<Diagnostic>, retry_after_seconds:Option<u32>, #[serde(default)] throttle_refusal:bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
enum Operation { Account, Evaluate, Lock, Redeem, Unlock }
impl Operation { fn name(self) -> &'static str { match self { Self::Account=>"account",Self::Evaluate=>"evaluate",Self::Lock=>"lock",Self::Redeem=>"redeem",Self::Unlock=>"unlock" } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
enum DiagnosticStage { Origin, Headers, Request, Http, BodyShape, BodyRead, BodySize, Json, AccountShape, ResultShape, ProductShape, Identifier, Echo }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
enum ValueKind { Missing, Null, Array, Object, String, Number, Boolean, Other }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Diagnostic {
    operation:Operation,stage:DiagnosticStage,
    #[serde(skip_serializing_if="Option::is_none")] http_status:Option<u16>,
    #[serde(skip_serializing_if="Option::is_none")] product_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] eula_ids_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] namespace_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] entitlement_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] title_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] code_use_id_kind:Option<ValueKind>,
    #[serde(skip_serializing_if="Option::is_none")] has_errors:Option<bool>,
    #[serde(skip_serializing_if="Option::is_none")] success:Option<bool>,
    #[serde(skip_serializing_if="Option::is_none")] service_code:Option<String>,
}
impl Diagnostic {fn valid(&self)->bool{self.http_status.is_none_or(|v|(100..=599).contains(&v))}}
struct Request<'a> { operation: Operation, code: &'a str, code_use_id: &'a str, locale: &'a str }
trait Transport {
    fn request<'a>(&'a mut self, request: Request<'a>) -> Pin<Box<dyn Future<Output=Result<Reply,Reason>> + Send + 'a>>;
    fn diagnostic(&self)->Option<Diagnostic>{None}
}
struct Draft { snapshot: Snapshot, keys: Vec<Option<Secret>>, previews: Vec<Option<Product>>, account_id: String, last_evaluate:Option<tokio::time::Instant>, evaluated_count:usize }
#[derive(Clone, Copy)]
struct PreparePolicy { budget:Duration, default_cooldown:Duration, server_second:Duration, max_cooldowns:usize, batch_size:usize, batch_pause:Duration }
impl Default for PreparePolicy {fn default()->Self{Self{budget:Duration::from_secs(20*60),default_cooldown:Duration::from_secs(60),server_second:Duration::from_secs(1),max_cooldowns:10,batch_size:10,batch_pause:Duration::from_secs(60)}}}
#[derive(Clone, Copy)]
struct ActivationPolicy { budget:Duration, default_cooldown:Duration, server_second:Duration, max_recoveries:usize, batch_size:usize, batch_pause:Duration, before_activation:Duration }
impl Default for ActivationPolicy {fn default()->Self{Self{budget:Duration::from_secs(20*60),default_cooldown:Duration::from_secs(60),server_second:Duration::from_secs(1),max_recoveries:10,batch_size:10,batch_pause:Duration::from_secs(60),before_activation:Duration::from_secs(60)}}}
fn exact_throttle_refusal(reply:&Reply)->bool{
    !reply.ok&&reply.throttle_refusal&&reply.reason==Some(Reason::RateLimited)&&reply.diagnostic.as_ref().is_some_and(|d|d.operation==Operation::Redeem&&d.stage==DiagnosticStage::ResultShape&&d.http_status==Some(200)&&d.has_errors==Some(true)&&d.success!=Some(true)&&d.service_code.as_deref().is_none_or(|code|code=="errors.com.epicgames.common.throttled"))
}
fn contains_ascii(value:&str,needle:&str)->bool{!needle.is_empty()&&value.as_bytes().windows(needle.len()).any(|part|part.eq_ignore_ascii_case(needle.as_bytes()))}
fn canonical_service_code(value:&str)->bool{value.strip_prefix("errors.com.epicgames.").is_some_and(|suffix|!suffix.is_empty()&&suffix.len()<=160&&suffix.bytes().all(|c|c.is_ascii_lowercase()||c==b'.'||c==b'_'))}
fn guarded_diagnostic(diagnostic:Option<Diagnostic>,draft:&Draft)->Option<Diagnostic>{
    let mut diagnostic=diagnostic.filter(Diagnostic::valid)?;
    if diagnostic.service_code.as_ref().is_some_and(|code|!canonical_service_code(code)||contains_ascii(code,&draft.account_id)||draft.keys.iter().flatten().any(|key|contains_ascii(code,key.text())||key.text().split('-').filter(|part|!part.is_empty()).any(|part|contains_ascii(code,part)))){diagnostic.service_code=None;}
    Some(diagnostic)
}
fn select_ready_rows(draft:&mut Draft){
    let items=std::mem::take(&mut draft.snapshot.items);let keys=std::mem::take(&mut draft.keys);let previews=std::mem::take(&mut draft.previews);
    for ((mut item,key),preview) in items.into_iter().zip(keys).zip(previews){if item.status==Status::Ready&&key.is_some()&&preview.is_some(){item.index=draft.snapshot.items.len();draft.snapshot.items.push(item);draft.keys.push(key);draft.previews.push(preview);}}
    draft.snapshot.preparation_stage=None;
}
fn mask(value: &str) -> String {
    if value.len() <= 4 { return "****".into(); }
    let segments:Vec<_>=value.split('-').collect();
    if segments.len()>=2 && segments.iter().all(|s|!s.is_empty()) { let last=segments.last().unwrap();return last[last.len().saturating_sub(8)..].into(); }
    format!("**** {}", &value[value.len()-4..])
}
fn input(keys: Vec<String>) -> Result<Draft,String> {
    if keys.is_empty() || keys.len()>256 || keys.iter().map(String::len).sum::<usize>()>65536 || keys.iter().any(|s|s.len()>128 || !s.is_ascii()) { return Err(INVALID.into()); }
    let mut secrets:Vec<Option<Secret>>=Vec::new(); let mut items=Vec::new(); let mut unique=0;
    for (index,key) in keys.into_iter().enumerate() {
        let mut bytes=key.into_bytes(); let start=bytes.iter().position(|b|!b.is_ascii_whitespace()).unwrap_or(bytes.len()); let end=bytes.iter().rposition(|b|!b.is_ascii_whitespace()).map(|v|v+1).unwrap_or(start); bytes.drain(end..); bytes.drain(..start);
        let value=Secret(bytes); let valid=!value.0.is_empty() && value.0.iter().all(|b|b.is_ascii_alphanumeric() || *b==b'-') && value.0.iter().any(u8::is_ascii_alphanumeric);
        let duplicate=valid && secrets.iter().flatten().any(|s|s.0.eq_ignore_ascii_case(&value.0));
        if valid && !duplicate { unique+=1; if unique>100 { return Err(INVALID.into()); } }
        items.push(Item { id:uuid::Uuid::new_v4().to_string(),index,masked_key:if valid {mask(value.text())} else {"****".into()},status:if valid&&!duplicate {Status::Pending}else{Status::Failed},title:String::new(),reason:if !valid {Some(Reason::InvalidInput)}else if duplicate {Some(Reason::Duplicate)}else{None},diagnostic:None });
        secrets.push(if valid&&!duplicate {Some(value)}else{None});
    }
    let count=items.len(); Ok(Draft { snapshot:Snapshot {batch_id:uuid::Uuid::new_v4().to_string(),account_name:String::new(),phase:Phase::Preparing,items,expires_at:None,wait_seconds:None,wait_reason:None,preparation_stage:None},keys:secrets,previews:vec![None;count],account_id:String::new(),last_evaluate:None,evaluated_count:0 })
}
fn supported(p:&Product) -> bool { !p.namespace.is_empty() && (if p.entitlement_name_is_null {p.entitlement_name.is_empty()}else{!p.entitlement_name.is_empty()}) && !p.title.is_empty() && p.title.chars().count()<=240 && !p.title.chars().any(char::is_control) && p.eula_ids.is_empty() && [&p.namespace,&p.offer_id,&p.entitlement_name,&p.code_use_id].iter().all(|s|s.len()<=128 && !s.chars().any(char::is_control)) }
fn same_product(a:&Product,b:&Product) -> bool {
    if a.namespace!=b.namespace{return false;}
    match (a.entitlement_name_is_null,b.entitlement_name_is_null){
        // Some successful official responses explicitly have no entitlement
        // ID. This remains the same immutable key and account; the fresh lock
        // must agree on namespace and the exact displayed product title.
        (true,true)=>a.entitlement_name.is_empty()&&b.entitlement_name.is_empty()&&a.title==b.title,
        (false,false)=>!a.entitlement_name.is_empty()&&a.entitlement_name==b.entitlement_name,
        _=>false,
    }
}
fn safe_title(product:&Product,key:&str)->bool {
    let folded=key.replace('-', "").to_ascii_uppercase();
    !folded.is_empty() && !product.title.replace('-', "").to_ascii_uppercase().contains(&folded)
}
fn stops_batch(reason:Reason)->bool {matches!(reason,Reason::Unknown|Reason::AuthRequired|Reason::AccountChanged|Reason::RequiresOfficial|Reason::Network|Reason::Service|Reason::RateLimited)}
fn redeem_refusal(reason:Option<Reason>)->Reason {
    // Only the bridge's explicit success:false + recognized service-code
    // classifications prove rejection. Opaque transport/service failures do not.
    match reason {Some(value @ (Reason::NotFound|Reason::AlreadyUsed|Reason::AlreadyOwned|Reason::Expired|Reason::RegionRestricted|Reason::AuthRequired))=>value,_=>Reason::Unknown}
}
fn failure(item:&mut Item,reason:Reason) { item.reason=Some(reason);item.status=if reason==Reason::Unknown {Status::Unknown}else if reason==Reason::RequiresOfficial {Status::RequiresOfficial}else{Status::Failed}; }
fn stop(draft:&mut Draft,reason:Reason) { draft.snapshot.phase=Phase::Stopped;draft.snapshot.wait_seconds=Some(0);draft.snapshot.wait_reason=None;for item in &mut draft.snapshot.items {if matches!(item.status,Status::Pending|Status::Ready|Status::Checking) {item.status=Status::NotAttempted;item.reason=Some(reason);} } }
fn abort_preparing(draft:&mut Draft,reason:Reason,cancel:&AtomicBool){
    if cancel.load(Ordering::SeqCst)||reason==Reason::Cancelled{stop(draft,Reason::Cancelled);return;}
    draft.snapshot.wait_seconds=Some(0);draft.snapshot.wait_reason=None;
    for item in &mut draft.snapshot.items{if matches!(item.status,Status::Pending|Status::Checking){item.status=Status::NotAttempted;item.reason=Some(reason);}}
    draft.snapshot.phase=if draft.snapshot.items.iter().any(|i|i.status==Status::Ready){Phase::Ready}else{Phase::Stopped};
}
async fn request_until(transport:&mut impl Transport,request:Request<'_>,deadline:tokio::time::Instant)->Result<Reply,Reason>{
    let timeout_reason=if matches!(request.operation,Operation::Redeem){Reason::Unknown}else{Reason::Network};
    if tokio::time::Instant::now()>=deadline{return Err(timeout_reason);}
    tokio::time::timeout_at(deadline,transport.request(request)).await.map_err(|_|timeout_reason)?
}
async fn check_account(transport:&mut impl Transport,expected:&str,deadline:tokio::time::Instant)->Result<(),Reason>{
    let r=request_until(transport,Request {operation:Operation::Account,code:"",code_use_id:"",locale:""},deadline).await?;
    if !r.ok{return Err(r.reason.unwrap_or(Reason::AuthRequired));}if r.account_id.as_deref()!=Some(expected){return Err(Reason::AccountChanged);}Ok(())
}
async fn countdown(draft:&mut Draft,cancel:&AtomicBool,delay:Duration,deadline:tokio::time::Instant,reason:WaitReason,notify:&mut impl FnMut(&Snapshot))->Result<(),Reason> {
    let until=tokio::time::Instant::now()+delay;
    loop {
        if cancel.load(Ordering::SeqCst){return Err(Reason::Cancelled);}
        let now=tokio::time::Instant::now();
        if now>=deadline{return Err(Reason::Network);}
        let remaining=until.saturating_duration_since(now);
        if remaining.is_zero(){break;}
        draft.snapshot.wait_seconds=Some(((remaining.as_millis()+999)/1000).min(3600) as u32);draft.snapshot.wait_reason=Some(reason);notify(&draft.snapshot);
        tokio::time::sleep(remaining.min(Duration::from_secs(1)).min(deadline.saturating_duration_since(now))).await;
    }
    if !delay.is_zero(){draft.snapshot.wait_seconds=Some(0);draft.snapshot.wait_reason=None;notify(&draft.snapshot);}Ok(())
}
async fn pace(draft:&mut Draft,cancel:&AtomicBool,delay:Duration,deadline:tokio::time::Instant,notify:&mut impl FnMut(&Snapshot))->bool{
    match countdown(draft,cancel,delay,deadline,WaitReason::Pacing,notify).await{Ok(())=>true,Err(reason)=>{stop(draft,reason);notify(&draft.snapshot);false}}
}
fn quota_delay(reply:&Reply,policy:PreparePolicy)->Option<Duration>{
    match reply.retry_after_seconds{Some(value @ 1..=120)=>Some(policy.server_second.saturating_mul(value)),Some(value) if value>120=>None,_=>Some(policy.default_cooldown)}
}
async fn prepare_core(draft:&mut Draft,transport:&mut impl Transport,cancel:&AtomicBool,locale:&str,delay:Duration,notify:impl FnMut(&Snapshot)){
    prepare_core_with_policy(draft,transport,cancel,locale,delay,PreparePolicy::default(),notify).await;
}
async fn prepare_core_with_policy(draft:&mut Draft,transport:&mut impl Transport,cancel:&AtomicBool,locale:&str,delay:Duration,policy:PreparePolicy,mut notify:impl FnMut(&Snapshot)) {
    let deadline=tokio::time::Instant::now()+policy.budget;
    let mut cooldowns=0;
    let mut processed=0;
    'rows: for index in 0..draft.keys.len() {
        if cancel.load(Ordering::SeqCst) {stop(draft,Reason::Cancelled);break;}
        if draft.keys[index].is_none(){continue;}
        let mut retried=false;
        let mut counted=false;
        loop{
            draft.snapshot.items[index].status=Status::Checking;draft.snapshot.items[index].reason=None;draft.snapshot.items[index].diagnostic=None;notify(&draft.snapshot);
            let answer={let key=draft.keys[index].as_ref().expect("validated key");request_until(transport,Request {operation:Operation::Evaluate,code:key.text(),code_use_id:"",locale},deadline).await};
            draft.last_evaluate=Some(tokio::time::Instant::now());
            if !counted{draft.evaluated_count+=1;counted=true;}
            let diagnostic=answer.as_ref().ok().and_then(|r|r.diagnostic.clone()).or_else(||transport.diagnostic());
            draft.snapshot.items[index].diagnostic=guarded_diagnostic(diagnostic,draft);
            if let Ok(reply)=&answer{if !reply.ok&&reply.reason==Some(Reason::RateLimited){
                draft.snapshot.items[index].reason=Some(Reason::RateLimited);
                let pause=quota_delay(reply,policy).filter(|duration|tokio::time::Instant::now().checked_add(*duration).is_some_and(|end|end<=deadline));
                if !retried&&cooldowns<policy.max_cooldowns {if let Some(duration)=pause{
                    retried=true;cooldowns+=1;
                    match countdown(draft,cancel,duration,deadline,WaitReason::RateLimited,&mut notify).await{
                        Ok(())=>continue,
                        Err(reason)=>{if reason==Reason::Cancelled||cancel.load(Ordering::SeqCst){abort_preparing(draft,Reason::Cancelled,cancel);}else{failure(&mut draft.snapshot.items[index],Reason::RateLimited);abort_preparing(draft,Reason::RateLimited,cancel);}notify(&draft.snapshot);break 'rows;}
                    }
                }}
                failure(&mut draft.snapshot.items[index],Reason::RateLimited);abort_preparing(draft,Reason::RateLimited,cancel);notify(&draft.snapshot);break 'rows;
            }}
            match answer {
                Ok(reply) if reply.ok => {
                    let product=reply.product.filter(|p|supported(p)&&safe_title(p,draft.keys[index].as_ref().expect("validated key").text()));
                    if let Some(p)=product{draft.snapshot.items[index].title=p.title.clone();draft.snapshot.items[index].status=Status::Ready;draft.snapshot.items[index].diagnostic=None;draft.previews[index]=Some(p);}
                    else{failure(&mut draft.snapshot.items[index],Reason::RequiresOfficial);abort_preparing(draft,Reason::RequiresOfficial,cancel);notify(&draft.snapshot);break 'rows;}
                },
                Ok(reply)=>{let reason=reply.reason.unwrap_or(Reason::Service);failure(&mut draft.snapshot.items[index],reason);if stops_batch(reason){abort_preparing(draft,reason,cancel);notify(&draft.snapshot);break 'rows;}},
                Err(reason)=>{failure(&mut draft.snapshot.items[index],reason);abort_preparing(draft,reason,cancel);notify(&draft.snapshot);break 'rows;}
            }
            break;
        }
        processed+=1;
        notify(&draft.snapshot);
        if draft.snapshot.items[index+1..].iter().any(|i|i.status==Status::Pending){let batch=policy.batch_size>0&&processed%policy.batch_size==0;let pause=if batch{policy.batch_pause}else{delay};let wait_reason=if batch{WaitReason::Batch}else{WaitReason::Pacing};if let Err(reason)=countdown(draft,cancel,pause,deadline,wait_reason,&mut notify).await{abort_preparing(draft,reason,cancel);notify(&draft.snapshot);break;}}
    }
    if draft.snapshot.phase!=Phase::Stopped {draft.snapshot.phase=if draft.snapshot.items.iter().any(|i|i.status==Status::Ready) {Phase::Ready}else{Phase::Completed};}notify(&draft.snapshot);
}
async fn key_request(draft:&Draft,index:usize,transport:&mut impl Transport,operation:Operation,code_use_id:&str,locale:&str,deadline:tokio::time::Instant)->Result<Reply,Reason>{
    request_until(transport,Request{operation,code:draft.keys[index].as_ref().expect("ready key").text(),code_use_id,locale},deadline).await
}
fn item_diagnostic(draft:&mut Draft,index:usize,transport:&impl Transport,answer:&Result<Reply,Reason>){
    draft.snapshot.items[index].diagnostic=guarded_diagnostic(answer.as_ref().ok().and_then(|r|r.diagnostic.clone()).or_else(||transport.diagnostic()),draft);
}
async fn activation_pause(draft:&mut Draft,index:usize,processed:usize,transport_delay:Duration,policy:ActivationPolicy,cancel:&AtomicBool,deadline:tokio::time::Instant,notify:&mut impl FnMut(&Snapshot))->bool{
    if !draft.snapshot.items[index+1..].iter().any(|i|i.status==Status::Ready){return true;}
    if policy.batch_size>0&&processed%policy.batch_size==0{
        match countdown(draft,cancel,policy.batch_pause,deadline,WaitReason::Batch,notify).await{Ok(())=>true,Err(reason)=>{stop(draft,reason);notify(&draft.snapshot);false}}
    }else{pace(draft,cancel,transport_delay,deadline,notify).await}
}
async fn recover_product(draft:&mut Draft,index:usize,old:&Product,preview:&Product,transport:&mut impl Transport,cancel:&AtomicBool,locale:&str,deadline:tokio::time::Instant)->Result<Product,Reason>{
    if cancel.load(Ordering::SeqCst){return Err(Reason::Cancelled);}
    if check_account(transport,&draft.account_id,deadline).await.is_err(){draft.snapshot.items[index].diagnostic=guarded_diagnostic(transport.diagnostic(),draft);return Err(Reason::Unknown);}
    if cancel.load(Ordering::SeqCst){return Err(Reason::Cancelled);}
    let evaluated=key_request(draft,index,transport,Operation::Evaluate,"",locale,deadline).await;
    draft.last_evaluate=Some(tokio::time::Instant::now());item_diagnostic(draft,index,transport,&evaluated);
    let valid=evaluated.ok().filter(|r|r.ok).and_then(|r|r.product).is_some_and(|p|supported(&p)&&same_product(preview,&p)&&safe_title(&p,draft.keys[index].as_ref().expect("ready key").text()));
    if !valid{return Err(Reason::Unknown);}
    // Only now is unused/unchanged state established. Never unlock an opaque
    // final outcome, or reuse its cached lock ID for a second redemption.
    let unlocked=key_request(draft,index,transport,Operation::Unlock,&old.code_use_id,locale,deadline).await;
    item_diagnostic(draft,index,transport,&unlocked);
    if !unlocked.is_ok_and(|r|r.ok){return Err(Reason::Unknown);}
    if cancel.load(Ordering::SeqCst){return Err(Reason::Cancelled);}
    let locked=key_request(draft,index,transport,Operation::Lock,"",locale,deadline).await;
    item_diagnostic(draft,index,transport,&locked);
    let product=locked.ok().filter(|r|r.ok).and_then(|r|r.product).ok_or(Reason::Unknown)?;
    if !supported(&product)||product.code_use_id.is_empty()||!same_product(preview,&product){if !product.code_use_id.is_empty(){let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;}return Err(Reason::Unknown);}
    if cancel.load(Ordering::SeqCst){let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;return Err(Reason::Cancelled);}
    if check_account(transport,&draft.account_id,deadline).await.is_err(){let diagnostic=guarded_diagnostic(transport.diagnostic(),draft);let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;draft.snapshot.items[index].diagnostic=diagnostic;return Err(Reason::Unknown);}
    if cancel.load(Ordering::SeqCst){let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;return Err(Reason::Cancelled);}
    Ok(product)
}
async fn activate_core(draft:&mut Draft,transport:&mut impl Transport,cancel:&AtomicBool,locale:&str,delay:Duration,notify:impl FnMut(&Snapshot)){
    activate_core_with_policy(draft,transport,cancel,locale,delay,ActivationPolicy::default(),notify).await;
}
async fn activate_core_with_policy(draft:&mut Draft,transport:&mut impl Transport,cancel:&AtomicBool,locale:&str,delay:Duration,policy:ActivationPolicy,mut notify:impl FnMut(&Snapshot)){
    select_ready_rows(draft);
    let deadline=tokio::time::Instant::now()+policy.budget;
    draft.snapshot.phase=Phase::Activating;draft.snapshot.expires_at=None;draft.snapshot.preparation_stage=None;notify(&draft.snapshot);
    if cancel.load(Ordering::SeqCst){stop(draft,Reason::Cancelled);notify(&draft.snapshot);return;}
    if policy.batch_size>0&&draft.evaluated_count>=policy.batch_size{if let Some(last)=draft.last_evaluate{let pause=policy.before_activation.saturating_sub(last.elapsed());if let Err(reason)=countdown(draft,cancel,pause,deadline,WaitReason::BeforeActivation,&mut notify).await{stop(draft,reason);notify(&draft.snapshot);return;}}}
    if let Err(reason)=check_account(transport,&draft.account_id,deadline).await{let diagnostic=guarded_diagnostic(transport.diagnostic(),draft);for item in &mut draft.snapshot.items{if item.status==Status::Ready{item.diagnostic=diagnostic.clone();}}stop(draft,reason);notify(&draft.snapshot);return;}
    let mut recoveries=0;let mut processed=0;
    'rows: for index in 0..draft.keys.len(){
        if cancel.load(Ordering::SeqCst){stop(draft,Reason::Cancelled);break;}
        if draft.snapshot.items[index].status!=Status::Ready{continue;}
        draft.snapshot.items[index].status=Status::Activating;draft.snapshot.items[index].reason=None;draft.snapshot.items[index].diagnostic=None;notify(&draft.snapshot);
        let preview=draft.previews[index].as_ref().expect("ready product").clone();
        let locked=key_request(draft,index,transport,Operation::Lock,"",locale,deadline).await;item_diagnostic(draft,index,transport,&locked);
        let product=match locked{Ok(reply) if reply.ok=>reply.product,Ok(reply)=>{let reason=reply.reason.unwrap_or(Reason::Service);failure(&mut draft.snapshot.items[index],reason);if stops_batch(reason){stop(draft,reason);notify(&draft.snapshot);break;}processed+=1;notify(&draft.snapshot);if !activation_pause(draft,index,processed,delay,policy,cancel,deadline,&mut notify).await{break;}continue;},Err(reason)=>{failure(&mut draft.snapshot.items[index],reason);stop(draft,reason);notify(&draft.snapshot);break;}};
        if !product.as_ref().is_some_and(|p|supported(p)&&!p.code_use_id.is_empty()&&same_product(&preview,p)){if let Some(p)=product.as_ref(){if !p.code_use_id.is_empty(){let _=key_request(draft,index,transport,Operation::Unlock,&p.code_use_id,locale,deadline).await;}}failure(&mut draft.snapshot.items[index],Reason::RequiresOfficial);processed+=1;notify(&draft.snapshot);if !activation_pause(draft,index,processed,delay,policy,cancel,deadline,&mut notify).await{break;}continue;}
        let mut product=product.expect("validated lock product");draft.snapshot.items[index].diagnostic=None;
        if cancel.load(Ordering::SeqCst){let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;draft.snapshot.items[index].status=Status::NotAttempted;draft.snapshot.items[index].reason=Some(Reason::Cancelled);stop(draft,Reason::Cancelled);notify(&draft.snapshot);break;}
        if let Err(reason)=check_account(transport,&draft.account_id,deadline).await{let diagnostic=guarded_diagnostic(transport.diagnostic(),draft);let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;draft.snapshot.items[index].diagnostic=diagnostic;failure(&mut draft.snapshot.items[index],reason);stop(draft,reason);notify(&draft.snapshot);break;}
        if cancel.load(Ordering::SeqCst){let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;draft.snapshot.items[index].status=Status::NotAttempted;draft.snapshot.items[index].reason=Some(Reason::Cancelled);stop(draft,Reason::Cancelled);notify(&draft.snapshot);break;}
        let mut recovered=false;
        loop{
            let result=key_request(draft,index,transport,Operation::Redeem,&product.code_use_id,locale,deadline).await;item_diagnostic(draft,index,transport,&result);
            match result{
                Ok(reply) if reply.ok=>{draft.snapshot.items[index].status=Status::Success;draft.snapshot.items[index].reason=None;draft.snapshot.items[index].diagnostic=None;break;},
                Ok(reply) if exact_throttle_refusal(&reply)=>{
                    let pause=(match reply.retry_after_seconds{Some(value @ 1..=120)=>Some(policy.server_second.saturating_mul(value)),Some(value) if value>120=>None,_=>Some(policy.default_cooldown)}).filter(|duration|tokio::time::Instant::now().checked_add(*duration).is_some_and(|end|end<=deadline));
                    if !recovered&&recoveries<policy.max_recoveries{if let Some(pause)=pause{
                        recovered=true;recoveries+=1;draft.snapshot.items[index].reason=Some(Reason::RateLimited);notify(&draft.snapshot);
                        if let Err(reason)=countdown(draft,cancel,pause,deadline,WaitReason::RateLimited,&mut notify).await{if reason==Reason::Cancelled{draft.snapshot.items[index].status=Status::NotAttempted;draft.snapshot.items[index].reason=Some(reason);}else{failure(&mut draft.snapshot.items[index],Reason::Unknown);}stop(draft,if reason==Reason::Cancelled{reason}else{Reason::Unknown});notify(&draft.snapshot);break 'rows;}
                        draft.snapshot.items[index].reason=None;draft.snapshot.items[index].diagnostic=None;notify(&draft.snapshot);
                        match recover_product(draft,index,&product,&preview,transport,cancel,locale,deadline).await{Ok(fresh)=>{product=fresh;draft.snapshot.items[index].diagnostic=None;continue;},Err(Reason::Cancelled)=>{draft.snapshot.items[index].status=Status::NotAttempted;draft.snapshot.items[index].reason=Some(Reason::Cancelled);stop(draft,Reason::Cancelled);notify(&draft.snapshot);break 'rows;},Err(_)=>{failure(&mut draft.snapshot.items[index],Reason::Unknown);stop(draft,Reason::Unknown);notify(&draft.snapshot);break 'rows;}}
                    }}
                    failure(&mut draft.snapshot.items[index],Reason::Unknown);stop(draft,Reason::Unknown);notify(&draft.snapshot);break 'rows;
                },
                Ok(reply)=>{let reason=redeem_refusal(reply.reason);failure(&mut draft.snapshot.items[index],reason);if reason==Reason::Unknown{stop(draft,reason);notify(&draft.snapshot);break 'rows;}let _=key_request(draft,index,transport,Operation::Unlock,&product.code_use_id,locale,deadline).await;if stops_batch(reason){stop(draft,reason);notify(&draft.snapshot);break 'rows;}break;},
                Err(_)=>{failure(&mut draft.snapshot.items[index],Reason::Unknown);stop(draft,Reason::Unknown);notify(&draft.snapshot);break 'rows;}
            }
        }
        processed+=1;notify(&draft.snapshot);if !activation_pause(draft,index,processed,delay,policy,cancel,deadline,&mut notify).await{break;}
    }
    if draft.snapshot.phase!=Phase::Stopped{draft.snapshot.phase=Phase::Completed;}notify(&draft.snapshot);
}
type Pending=Arc<Mutex<Option<(String,oneshot::Sender<Reply>)>>>;
struct Bridge { window:WebviewWindow, pending:Pending, diagnostic:Option<Diagnostic> }
impl Drop for Bridge { fn drop(&mut self) {let _=self.window.destroy();} }
impl Bridge {
    async fn open(app:&tauri::AppHandle,session:super::epic::WebSession)->Result<Self,Reason> {
        let url=super::epic_redeem::redemption_url(crate::ui_locale::language(),session.exchange_code.as_deref()).map_err(|_|Reason::RequiresOfficial)?;
        let main=app.get_webview_window("main").ok_or(Reason::Service)?;let nonce=uuid::Uuid::new_v4().simple().to_string();let target=format!("/um-redeem-batch/{nonce}");
        let callback=serde_json::to_string(&format!("https://localhost{target}")).map_err(|_|Reason::Service)?;
        let script=include_str!("epic_redeem_batch_bridge.js").replace("__CALLBACK__",&callback);let(tx,rx)=oneshot::channel();let pending:Pending=Arc::new(Mutex::new(Some(("boot".into(),tx))));let results=pending.clone();
        let window=WebviewWindowBuilder::new(app,LABEL,WebviewUrl::External(url)).additional_browser_args(crate::webview_policy::BROWSER_ARGUMENTS).data_directory(super::epic_browser::profile(app).map_err(|_|Reason::Service)?).incognito(!session.remember).visible(false).skip_taskbar(true).owner(&main).map_err(|_|Reason::Service)?.initialization_script(&script).on_navigation(move|url|{
            if url.host_str()==Some("localhost") {if url.scheme()=="https"&&url.port_or_known_default()==Some(443)&&url.path()==target {if let Some(raw)=url.query_pairs().find(|(k,_)|k=="result").map(|(_,v)|v) {if raw.len()<=16384 {if let Ok(reply)=serde_json::from_str::<Reply>(&raw) {if let Ok(mut slot)=results.lock() {if slot.as_ref().is_some_and(|(id,_)|*id==reply.id) {if let Some((_,sender))=slot.take(){let _=sender.send(reply);}}}}}}}return false;}
            url.scheme()=="https"&&url.host_str().is_some_and(|host|host=="epicgames.com"||host.ends_with(".epicgames.com"))
        }).build().map_err(|_|Reason::RequiresOfficial)?;
        let closed=pending.clone();window.on_window_event(move|event|{if matches!(event,WindowEvent::Destroyed){if let Ok(mut slot)=closed.lock(){slot.take();}}});
        let bridge=Self{window,pending,diagnostic:None};match tokio::time::timeout(Duration::from_secs(45),rx).await {Ok(Ok(reply)) if reply.ok=>Ok(bridge),_=>Err(Reason::RequiresOfficial)}
    }
}
impl Transport for Bridge {
    fn diagnostic(&self)->Option<Diagnostic>{self.diagnostic.clone()}
    fn request<'a>(&'a mut self,request:Request<'a>)->Pin<Box<dyn Future<Output=Result<Reply,Reason>>+Send+'a>> {Box::pin(async move {
        self.diagnostic=None;
        #[derive(Serialize)]#[serde(rename_all="camelCase")]struct Args<'a>{code:&'a str,code_use_id:&'a str,locale:&'a str}
        let id=uuid::Uuid::new_v4().to_string();let(tx,rx)=oneshot::channel();{let mut slot=self.pending.lock().map_err(|_|Reason::Service)?;if slot.is_some(){return Err(Reason::Service);}*slot=Some((id.clone(),tx));}
        let mut script=Secret(b"window.__umRedeemBatch(...".to_vec());serde_json::to_writer(&mut script.0,&(&id,request.operation.name(),Args{code:request.code,code_use_id:request.code_use_id,locale:request.locale})).map_err(|_|Reason::Service)?;script.0.extend_from_slice(b");");let evaluated=self.window.eval(std::str::from_utf8(&script.0).map_err(|_|Reason::Service)?);drop(script);if evaluated.is_err(){return Err(Reason::RequiresOfficial);}
        match tokio::time::timeout(Duration::from_secs(28),rx).await {Ok(Ok(reply))=>{self.diagnostic=reply.diagnostic.clone().filter(Diagnostic::valid);Ok(reply)},_=>{let _=self.window.destroy();Err(Reason::Network)}}
    })}
}

struct Batch { draft:Draft, bridge:Bridge, _flow:super::epic::WebFlowGuard, _operation:crate::app_updates::Operation, _expiry:oneshot::Sender<()>, channel:tauri::ipc::Channel<Snapshot> }
struct Current {id:String,cancel:Arc<AtomicBool>,phase:Phase}
#[derive(Default)]struct Registry {current:Option<Current>,ready:Option<Batch>}
fn registry()->&'static Mutex<Registry>{static STORE:OnceLock<Mutex<Registry>>=OnceLock::new();STORE.get_or_init(||Mutex::new(Registry::default()))}
struct Lease {id:String,retained:bool}
impl Drop for Lease {fn drop(&mut self){if !self.retained {finish(&self.id);}}}
fn finish(id:&str){if let Ok(mut state)=registry().lock(){if state.current.as_ref().is_some_and(|v|v.id==id){state.current=None;ACTIVE.store(false,Ordering::SeqCst);}}}
fn phase(id:&str,value:Phase){if let Ok(mut state)=registry().lock(){if let Some(current)=state.current.as_mut().filter(|v|v.id==id){current.phase=value;}}}
fn begin(id:&str)->Result<(Lease,Arc<AtomicBool>),String>{let mut state=registry().lock().map_err(|_|BUSY)?;if state.current.is_some(){return Err(BUSY.into());}let cancel=Arc::new(AtomicBool::new(false));state.current=Some(Current{id:id.into(),cancel:cancel.clone(),phase:Phase::Preparing});ACTIVE.store(true,Ordering::SeqCst);Ok((Lease{id:id.into(),retained:false},cancel))}
fn now()->u64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()}
fn publish(channel:&tauri::ipc::Channel<Snapshot>,snapshot:&Snapshot){phase(&snapshot.batch_id,snapshot.phase);let _=channel.send(snapshot.clone());}
// Release a preview immediately instead of making normal shutdown wait for its
// five-minute expiry. An in-flight request is allowed to report its one result;
// the cancellation flag prevents dispatching the next request or key.
pub(crate) fn stop_for_shutdown(){
    let ready={let mut store=match registry().lock(){Ok(s)=>s,Err(_)=>return};if let Some(current)=store.current.as_ref(){current.cancel.store(true,Ordering::SeqCst);}store.ready.take()};
    if let Some(mut batch)=ready{let id=batch.draft.snapshot.batch_id.clone();stop(&mut batch.draft,Reason::Cancelled);publish(&batch.channel,&batch.draft.snapshot);drop(batch);finish(&id);}
}

#[tauri::command]
pub(crate) async fn epic_redeem_batch_prepare(app:tauri::AppHandle,state:tauri::State<'_,super::epic::EpicState>,keys:Vec<String>,on_progress:tauri::ipc::Channel<Snapshot>)->Result<Snapshot,String>{
    let mut draft=input(keys)?;if draft.keys.iter().all(Option::is_none){return Err(INVALID.into());}let id=draft.snapshot.batch_id.clone();let(mut lease,cancel)=begin(&id)?;let operation=crate::app_updates::operation()?;draft.snapshot.preparation_stage=Some(PreparationStage::Account);publish(&on_progress,&draft.snapshot);let flow=super::epic::reserve_web_flow()?;
    let(account_id,account_name)=super::epic::web_account_binding(&app,&state).await?;draft.account_id=account_id;draft.snapshot.account_name=account_name;
    if cancel.load(Ordering::SeqCst){stop(&mut draft,Reason::Cancelled);publish(&on_progress,&draft.snapshot);return Ok(draft.snapshot);}
    draft.snapshot.preparation_stage=Some(PreparationStage::Session);publish(&on_progress,&draft.snapshot);
    let session=super::epic::prepare_web_session(&app,&state).await?;
    draft.snapshot.preparation_stage=Some(PreparationStage::Connection);publish(&on_progress,&draft.snapshot);
    let mut bridge=match Bridge::open(&app,session).await {Ok(bridge)=>bridge,Err(reason)=>{for item in &mut draft.snapshot.items {if item.status==Status::Pending {failure(item,reason);item.status=Status::RequiresOfficial;}}draft.snapshot.phase=Phase::Stopped;publish(&on_progress,&draft.snapshot);return Ok(draft.snapshot);}};
    if cancel.load(Ordering::SeqCst){stop(&mut draft,Reason::Cancelled);publish(&on_progress,&draft.snapshot);return Ok(draft.snapshot);}
    if let Err(reason)=check_account(&mut bridge,&draft.account_id,tokio::time::Instant::now()+Duration::from_secs(SESSION_SECONDS)).await {let diagnostic=guarded_diagnostic(bridge.diagnostic(),&draft);for item in &mut draft.snapshot.items {if item.status==Status::Pending {failure(item,reason);item.diagnostic=diagnostic.clone();}}draft.snapshot.phase=Phase::Stopped;publish(&on_progress,&draft.snapshot);return Ok(draft.snapshot);}
    draft.snapshot.preparation_stage=Some(PreparationStage::Keys);publish(&on_progress,&draft.snapshot);
    prepare_core(&mut draft,&mut bridge,&cancel,crate::ui_locale::language(),INTER_REQUEST_DELAY,|s|publish(&on_progress,s)).await;
    if draft.snapshot.phase!=Phase::Ready{return Ok(draft.snapshot);}
    draft.snapshot.expires_at=Some(now()+PREVIEW_SECONDS);publish(&on_progress,&draft.snapshot);let snapshot=draft.snapshot.clone();
    let(expiry_tx,expiry_rx)=oneshot::channel();
    {let mut store=registry().lock().map_err(|_|BUSY)?;if cancel.load(Ordering::SeqCst){drop(store);stop(&mut draft,Reason::Cancelled);publish(&on_progress,&draft.snapshot);return Ok(draft.snapshot);}store.ready=Some(Batch{draft,bridge,_flow:flow,_operation:operation,_expiry:expiry_tx,channel:on_progress});}lease.retained=true;
    tauri::async_runtime::spawn(async move{if tokio::time::timeout(Duration::from_secs(PREVIEW_SECONDS),expiry_rx).await.is_ok(){return;}let expired={let mut store=match registry().lock(){Ok(s)=>s,Err(_)=>return};if store.ready.as_ref().is_some_and(|b|b.draft.snapshot.batch_id==id){store.ready.take()}else{None}};if let Some(mut batch)=expired {stop(&mut batch.draft,Reason::PreviewExpired);publish(&batch.channel,&batch.draft.snapshot);drop(batch);finish(&id);}});
    Ok(snapshot)
}
#[tauri::command]
pub(crate) async fn epic_redeem_batch_activate(app:tauri::AppHandle,state:tauri::State<'_,super::epic::EpicState>,batch_id:String,on_progress:tauri::ipc::Channel<Snapshot>)->Result<Snapshot,String>{
    let(mut batch,cancel)={let mut store=registry().lock().map_err(|_|BUSY)?;if !store.ready.as_ref().is_some_and(|b|b.draft.snapshot.batch_id==batch_id){return Err(STALE.into());}let cancel=store.current.as_ref().ok_or(STALE)?.cancel.clone();(store.ready.take().ok_or(STALE)?,cancel)};
    let _lease=Lease{id:batch_id.clone(),retained:false};batch.channel=on_progress;
    select_ready_rows(&mut batch.draft);
    if batch.draft.snapshot.expires_at.is_none_or(|time|now()>=time){stop(&mut batch.draft,Reason::PreviewExpired);publish(&batch.channel,&batch.draft.snapshot);let result=batch.draft.snapshot.clone();drop(batch);return Ok(result);}
    match super::epic::web_account_binding(&app,&state).await {Ok((id,_)) if id==batch.draft.account_id=>{},_=>{stop(&mut batch.draft,Reason::AccountChanged);publish(&batch.channel,&batch.draft.snapshot);let result=batch.draft.snapshot.clone();drop(batch);return Ok(result);}}
    activate_core(&mut batch.draft,&mut batch.bridge,&cancel,crate::ui_locale::language(),INTER_REQUEST_DELAY,|s|publish(&batch.channel,s)).await;let result=batch.draft.snapshot.clone();drop(batch);Ok(result)
}
#[tauri::command]
pub(crate) fn epic_redeem_batch_cancel(batch_id:String)->bool{
    let ready={let mut store=match registry().lock(){Ok(s)=>s,Err(_)=>return false};let Some(current)=store.current.as_ref().filter(|v|v.id==batch_id)else{return false};current.cancel.store(true,Ordering::SeqCst);store.ready.take()};
    if let Some(mut batch)=ready {stop(&mut batch.draft,Reason::Cancelled);publish(&batch.channel,&batch.draft.snapshot);drop(batch);finish(&batch_id);}true
}
#[tauri::command]
pub(crate) fn epic_redeem_batch_discard(batch_id:String)->Result<(),String>{
    let ready={let mut store=registry().lock().map_err(|_|BUSY)?;if store.current.as_ref().is_some_and(|v|v.id==batch_id)&&store.ready.is_none(){return Err(RUNNING.into());}if store.ready.as_ref().is_some_and(|v|v.draft.snapshot.batch_id==batch_id){store.ready.take()}else{None}};let released=ready.is_some();drop(ready);if released{finish(&batch_id);}Ok(())
}

#[cfg(test)]
#[path="epic_redeem_batch_tests.rs"]
mod tests;

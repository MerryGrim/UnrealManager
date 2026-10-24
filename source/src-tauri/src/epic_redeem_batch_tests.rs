//! Synthetic transport fixtures only: no WebView, account, cookie, key, or API.
use super::*;
use std::collections::VecDeque;

const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210";
const KEY: &str = "SYNTHETIC-alpha-12345";
const KEY2: &str = "SYNTHETIC-beta-67890";

fn product() -> Product {
    Product { namespace:"fixture-namespace".into(),offer_id:"fixture-offer".into(),
        entitlement_name:"fixture-entitlement".into(),entitlement_name_is_null:false,title:"Synthetic product".into(),
        eula_ids:Vec::new(),code_use_id:"fixture-lock-id".into() }
}
fn nullable_product()->Product {Product{entitlement_name:String::new(),entitlement_name_is_null:true,..product()}}
fn reply(product:Option<Product>)->Reply {
    Reply{id:String::new(),ok:true,reason:None,product,account_id:None,account_name:None,diagnostic:None,retry_after_seconds:None,throttle_refusal:false}
}
fn account(id:&str)->Reply {let mut r=reply(None);r.account_id=Some(id.into());r}
fn rejected(reason:Reason)->Reply {let mut r=reply(None);r.ok=false;r.reason=Some(reason);r}
fn draft(keys:&[&str])->Draft {
    let mut draft=input(keys.iter().map(|s|(*s).into()).collect()).unwrap();
    draft.account_id=ACCOUNT.into();draft.snapshot.account_name="Synthetic account".into();draft
}
fn ready(keys:&[&str])->Draft {
    let mut draft=draft(keys);draft.snapshot.phase=Phase::Ready;draft.evaluated_count=keys.len();
    for index in 0..draft.keys.len(){draft.snapshot.items[index].status=Status::Ready;draft.previews[index]=Some(product());}
    draft
}
struct Fake<'a> {
    steps:VecDeque<(Operation,Result<Reply,Reason>)>,
    calls:Vec<(Operation,String,String)>,
    call_times:Vec<tokio::time::Instant>,
    cancel_on:Option<(Operation,&'a AtomicBool)>,
}
impl<'a> Fake<'a> {
    fn new(steps:Vec<(Operation,Result<Reply,Reason>)>)->Self {Self{steps:steps.into(),calls:Vec::new(),call_times:Vec::new(),cancel_on:None}}
    fn done(&self){assert!(self.steps.is_empty(),"Every scripted request must be consumed");}
    fn operations(&self)->Vec<Operation>{self.calls.iter().map(|c|c.0).collect()}
}
impl Transport for Fake<'_> {
    fn request<'a>(&'a mut self,request:Request<'a>)->Pin<Box<dyn Future<Output=Result<Reply,Reason>>+Send+'a>> {
        self.calls.push((request.operation,request.code.into(),request.code_use_id.into()));
        self.call_times.push(tokio::time::Instant::now());
        let(expected,result)=self.steps.pop_front().expect("Unexpected API request");
        assert_eq!(expected,request.operation,"Unexpected API order");
        if let Some((operation,cancel))=self.cancel_on {if operation==request.operation {cancel.store(true,Ordering::SeqCst);}}
        Box::pin(async move{result})
    }
}
fn ok(operation:Operation,value:Reply)->(Operation,Result<Reply,Reason>){(operation,Ok(value))}

#[test]
fn input_preserves_original_case_and_structure_and_deduplicates_only_case() {
    let d=draft(&[" \tAb-Cd-12345\r\n","ab-cd-12345","AbCd12345"]);
    assert_eq!(d.keys[0].as_ref().unwrap().text(),"Ab-Cd-12345");
    assert!(d.keys[1].is_none());assert_eq!(d.snapshot.items[1].reason,Some(Reason::Duplicate));
    assert_eq!(d.keys[2].as_ref().unwrap().text(),"AbCd12345");
}

#[test]
fn input_bounds_and_invalid_rows_do_not_become_requests() {
    for keys in [Vec::new(),vec!["a".repeat(129)],vec!["é".into()],vec!["A".into();257],vec!["x".repeat(128);256].into_iter().chain(vec!["x".repeat(128);256]).collect()] {
        assert!(input(keys).is_err());
    }
}

#[test]
fn all_hyphens_and_internal_whitespace_are_invalid_local_rows() {
    let d=draft(&["---","","WITH SPACE"]);
    assert!(d.keys.iter().all(Option::is_none));
    assert!(d.snapshot.items.iter().all(|i|i.reason==Some(Reason::InvalidInput)&&i.masked_key=="****"));
    assert!(input(vec!["\u{feff}BOM".into()]).is_err());
    assert!(input((0..101).map(|n|format!("UNIQUE-{n}")).collect()).is_err());
}

#[test]
fn masking_never_reveals_short_or_empty_segment_keys() {
    for value in ["A","ABCD","A-B","-ABC"]{assert_eq!(mask(value),"****");}
    assert_eq!(mask("-ABCDE"),"**** BCDE");assert_eq!(mask("ABCDE-"),"**** CDE-");
    assert_eq!(mask("AB--CDE"),"**** -CDE");assert_eq!(mask(KEY),"12345");
    assert_eq!(mask("PREFIX-1234567890"),"34567890");
}

#[test]
fn snapshots_cannot_serialize_raw_keys_or_native_account_binding() {
    let d=draft(&[KEY,KEY2]);let json=serde_json::to_string(&d.snapshot).unwrap();
    assert!(!json.contains(KEY));assert!(!json.contains(KEY2));assert!(!json.contains(ACCOUNT));
    assert!(json.contains("12345"));assert!(json.contains("Synthetic account"));
}

#[tokio::test]
async fn prepare_only_evaluates_and_never_locks_or_redeems() {
    let mut d=draft(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Evaluate,reply(Some(product()))),ok(Operation::Evaluate,reply(Some(product())))]);
    let mut observed=Vec::new();prepare_core(&mut d,&mut api,&cancel,"ru",Duration::ZERO,|s|observed.push(s.clone())).await;
    api.done();assert_eq!(api.operations(),vec![Operation::Evaluate,Operation::Evaluate]);assert_eq!(d.snapshot.phase,Phase::Ready);
    assert!(d.snapshot.items.iter().all(|i|i.status==Status::Ready));
    assert!(observed.iter().any(|s|s.items[0].status==Status::Checking));
}

#[tokio::test]
async fn eula_unknown_identity_and_key_echo_require_official_flow() {
    for p in [Product{eula_ids:vec!["must-accept".into()],..product()},Product{entitlement_name:String::new(),..product()},Product{title:format!("Echo {KEY}"),..product()}] {
        let mut d=draft(&[KEY]);let cancel=AtomicBool::new(false);let mut api=Fake::new(vec![ok(Operation::Evaluate,reply(Some(p)))]);
        prepare_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
        assert_eq!(d.snapshot.items[0].status,Status::RequiresOfficial);assert!(d.previews[0].is_none());
        assert!(!serde_json::to_string(&d.snapshot).unwrap().contains(KEY));
    }
}

#[tokio::test]
async fn explicit_activation_is_sequential_and_checks_account_after_each_lock() {
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut steps=vec![ok(Operation::Account,account(ACCOUNT))];
    for _ in 0..2 {steps.extend([ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,reply(None))]);}
    let mut api=Fake::new(steps);activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert_eq!(api.operations(),vec![Operation::Account,Operation::Lock,Operation::Account,Operation::Redeem,Operation::Lock,Operation::Account,Operation::Redeem]);
    assert_eq!(api.calls[3].1,KEY);assert_eq!(api.calls[6].1,KEY2);assert_eq!(api.calls[3].2,"fixture-lock-id");
    assert_eq!(d.snapshot.phase,Phase::Completed);assert!(d.snapshot.items.iter().all(|i|i.status==Status::Success));
}

#[tokio::test]
async fn changed_account_before_lock_sends_no_mutation() {
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);let mut api=Fake::new(vec![ok(Operation::Account,account(OTHER))]);
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();assert_eq!(d.snapshot.phase,Phase::Stopped);
    assert!(d.snapshot.items.iter().all(|i|i.status==Status::NotAttempted&&i.reason==Some(Reason::AccountChanged)));
}

#[tokio::test]
async fn account_changed_after_lock_releases_known_lock_without_redeem() {
    for p in [product(),nullable_product()]{
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    d.previews[0]=Some(p.clone());
    let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(p))),ok(Operation::Account,account(OTHER)),ok(Operation::Unlock,reply(None))]);
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();assert_eq!(d.snapshot.phase,Phase::Stopped);
    assert_eq!(d.snapshot.items[0].reason,Some(Reason::AccountChanged));assert_eq!(d.snapshot.items[1].status,Status::NotAttempted);
    assert!(!api.operations().contains(&Operation::Redeem));
    }
}

#[tokio::test]
async fn changed_product_releases_lock_and_never_redeems() {
    for altered in [Product{entitlement_name:"other".into(),..product()},Product{eula_ids:vec!["explicit-consent-needed".into()],..product()}]{
    let mut d=ready(&[KEY]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(altered))),ok(Operation::Unlock,reply(None))]);
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();assert_eq!(d.snapshot.items[0].status,Status::RequiresOfficial);
    assert!(!api.operations().contains(&Operation::Redeem));
    }
}

#[tokio::test]
async fn service_auth_network_and_unknown_lock_errors_stop_remaining_keys() {
    for reason in [Reason::Service,Reason::AuthRequired,Reason::Network,Reason::Unknown,Reason::RequiresOfficial,Reason::RateLimited] {
        let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
        let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,rejected(reason))]);
        activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
        assert_eq!(d.snapshot.phase,Phase::Stopped);assert_eq!(d.snapshot.items[1].status,Status::NotAttempted);
    }
}

#[tokio::test]
async fn opaque_redeem_result_is_unknown_and_never_unlocked_or_replayed() {
    for answer in [Ok(rejected(Reason::Unknown)),Ok(rejected(Reason::Service)),Ok(rejected(Reason::Network)),Ok(rejected(Reason::RequiresOfficial)),Ok(rejected(Reason::RateLimited)),Err(Reason::Network)] {
        let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
        let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),(Operation::Redeem,answer)]);
        activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
        assert_eq!(d.snapshot.items[0].status,Status::Unknown);assert_eq!(d.snapshot.items[1].status,Status::NotAttempted);
        assert_eq!(api.operations().iter().filter(|o|**o==Operation::Redeem).count(),1);
        assert!(!api.operations().contains(&Operation::Unlock));
    }
}

#[tokio::test]
async fn known_redeem_refusal_releases_lock_and_can_process_next_key() {
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,rejected(Reason::AlreadyUsed)),ok(Operation::Unlock,reply(None)),ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,reply(None))]);
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert_eq!(d.snapshot.items[0].reason,Some(Reason::AlreadyUsed));assert_eq!(d.snapshot.items[1].status,Status::Success);
}

#[tokio::test]
async fn cancel_before_prepare_dispatches_nothing() {
    let mut d=draft(&[KEY]);let cancel=AtomicBool::new(true);let mut api=Fake::new(vec![]);
    prepare_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();assert!(api.calls.is_empty());assert_eq!(d.snapshot.items[0].status,Status::NotAttempted);
}

#[tokio::test]
async fn cancel_during_lock_unlocks_before_any_redeem() {
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(product()))),ok(Operation::Unlock,reply(None))]);api.cancel_on=Some((Operation::Lock,&cancel));
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert!(!api.operations().contains(&Operation::Redeem));assert!(d.snapshot.items.iter().all(|i|i.status==Status::NotAttempted));
}

#[tokio::test]
async fn cancel_after_sent_redeem_keeps_its_result_and_stops_next_key() {
    let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,reply(None))]);api.cancel_on=Some((Operation::Redeem,&cancel));
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert_eq!(d.snapshot.items[0].status,Status::Success);assert_eq!(d.snapshot.items[1].status,Status::NotAttempted);assert_eq!(d.snapshot.phase,Phase::Stopped);
}

#[tokio::test]
async fn expired_budget_never_dispatches_and_distinguishes_sent_result_uncertainty() {
    let mut api=Fake::new(vec![]);let deadline=tokio::time::Instant::now()-Duration::from_secs(1);
    for operation in [Operation::Evaluate,Operation::Redeem] {
        let result=request_until(&mut api,Request{operation,code:KEY,code_use_id:"fixture",locale:"en"},deadline).await;
        assert_eq!(result.err(),Some(if operation==Operation::Redeem{Reason::Unknown}else{Reason::Network}));
    }
    assert!(api.calls.is_empty());
}

#[tokio::test]
async fn configured_pacing_waits_between_requests_once_and_reports_no_trailing_wait() {
    assert_eq!(INTER_REQUEST_DELAY,Duration::from_secs(2));
    let delay=Duration::from_millis(15);let mut d=draft(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Evaluate,reply(Some(product()))),ok(Operation::Evaluate,reply(Some(product())))]);
    let mut waits=Vec::new();prepare_core(&mut d,&mut api,&cancel,"en",delay,|s|{if s.wait_seconds==Some(1){waits.push(s.clone());}}).await;
    api.done();assert!(api.call_times[1].duration_since(api.call_times[0])>=delay);
    assert_eq!(waits.len(),1);assert_eq!(waits[0].items[0].status,Status::Ready);assert_eq!(waits[0].items[1].status,Status::Pending);
    assert_eq!(d.snapshot.wait_seconds,Some(0));assert_eq!(d.snapshot.phase,Phase::Ready);
}

#[tokio::test]
async fn cancel_during_preview_pacing_sends_no_next_request() {
    let mut d=draft(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Evaluate,reply(Some(product())))]);
    prepare_core(&mut d,&mut api,&cancel,"en",Duration::from_millis(10),|s|{if s.wait_seconds==Some(1){cancel.store(true,Ordering::SeqCst);}}).await;
    api.done();assert_eq!(d.snapshot.phase,Phase::Stopped);assert_eq!(d.snapshot.wait_seconds,Some(0));
    assert_eq!(d.snapshot.items[0].status,Status::NotAttempted);assert_eq!(d.snapshot.items[1].status,Status::NotAttempted);
    assert_eq!(api.calls.len(),1);
}

#[tokio::test]
async fn activation_pacing_preserves_completed_row_and_has_no_final_delay() {
    let delay=Duration::from_millis(15);let mut d=ready(&[KEY,KEY2]);let cancel=AtomicBool::new(false);
    let mut steps=vec![ok(Operation::Account,account(ACCOUNT))];
    for _ in 0..2{steps.extend([ok(Operation::Lock,reply(Some(product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,reply(None))]);}
    let mut api=Fake::new(steps);let mut waits=Vec::new();
    activate_core(&mut d,&mut api,&cancel,"en",delay,|s|{if s.wait_seconds==Some(1){waits.push(s.clone());}}).await;
    api.done();assert!(api.call_times[4].duration_since(api.call_times[3])>=delay);
    assert_eq!(waits.len(),1);assert_eq!(waits[0].items[0].status,Status::Success);assert_eq!(waits[0].items[1].status,Status::Ready);
    assert_eq!(d.snapshot.wait_seconds,Some(0));assert_eq!(d.snapshot.phase,Phase::Completed);
}

#[test]
fn diagnostics_have_no_free_form_or_secret_fields() {
    let closed=serde_json::json!({"operation":"evaluate","stage":"productShape","httpStatus":200,"eulaIdsKind":"null","productKind":"object","hasErrors":false,"success":true});
    let d:Diagnostic=serde_json::from_value(closed.clone()).unwrap();assert!(d.valid());
    let encoded=serde_json::to_string(&d).unwrap();assert!(!encoded.contains(KEY));assert!(!encoded.contains(ACCOUNT));
    let mut with_key=closed.clone();with_key["key"]=KEY.into();assert!(serde_json::from_value::<Diagnostic>(with_key).is_err());
    let mut with_text=closed.clone();with_text["stage"]=KEY.into();assert!(serde_json::from_value::<Diagnostic>(with_text).is_err());
    let mut bad_status=closed;bad_status["httpStatus"]=999.into();assert!(!serde_json::from_value::<Diagnostic>(bad_status).unwrap().valid());
}

#[tokio::test]
async fn unsupported_response_preserves_only_closed_shape_diagnostic() {
    let mut d=draft(&[KEY]);let cancel=AtomicBool::new(false);let mut answer=rejected(Reason::RequiresOfficial);
    answer.diagnostic=Some(serde_json::from_value(serde_json::json!({"operation":"evaluate","stage":"productShape","httpStatus":200,"eulaIdsKind":"missing","productKind":"object","hasErrors":false,"success":true})).unwrap());
    let mut api=Fake::new(vec![ok(Operation::Evaluate,answer)]);
    prepare_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert_eq!(d.snapshot.items[0].status,Status::RequiresOfficial);
    let diagnostic=d.snapshot.items[0].diagnostic.as_ref().unwrap();assert_eq!(diagnostic.stage,DiagnosticStage::ProductShape);assert_eq!(diagnostic.eula_ids_kind,Some(ValueKind::Missing));
    let rendered=serde_json::to_string(&d.snapshot).unwrap();assert!(!rendered.contains(KEY));assert!(!rendered.contains(ACCOUNT));
}

#[tokio::test]
async fn explicit_null_entitlement_preview_and_fresh_lock_can_activate_same_confirmed_product() {
    let mut d=draft(&[KEY]);let cancel=AtomicBool::new(false);
    let mut api=Fake::new(vec![ok(Operation::Evaluate,reply(Some(nullable_product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(nullable_product()))),ok(Operation::Account,account(ACCOUNT)),ok(Operation::Redeem,reply(None))]);
    prepare_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;
    assert_eq!(d.snapshot.phase,Phase::Ready);assert_eq!(d.snapshot.items[0].title,"Synthetic product");
    activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
    assert_eq!(d.snapshot.items[0].status,Status::Success);
    assert_eq!(api.operations(),vec![Operation::Evaluate,Operation::Account,Operation::Lock,Operation::Account,Operation::Redeem]);
    assert_eq!(api.calls[0].1,KEY);assert_eq!(api.calls[2].1,KEY);assert_eq!(api.calls[4].1,KEY);
    assert_eq!(api.calls[4].2,"fixture-lock-id");
}

#[tokio::test]
async fn nullable_entitlement_requires_both_null_same_namespace_and_exact_title() {
    for (preview,locked) in [
        (nullable_product(),Product{title:"Different product".into(),..nullable_product()}),
        (nullable_product(),Product{title:"synthetic product".into(),..nullable_product()}),
        (nullable_product(),Product{namespace:"other-namespace".into(),..nullable_product()}),
        (nullable_product(),product()),(product(),nullable_product()),
    ]{
        let mut d=ready(&[KEY]);d.previews[0]=Some(preview);let cancel=AtomicBool::new(false);
        let mut api=Fake::new(vec![ok(Operation::Account,account(ACCOUNT)),ok(Operation::Lock,reply(Some(locked))),ok(Operation::Unlock,reply(None))]);
        activate_core(&mut d,&mut api,&cancel,"en",Duration::ZERO,|_|{}).await;api.done();
        assert_eq!(d.snapshot.items[0].status,Status::RequiresOfficial);assert!(!api.operations().contains(&Operation::Redeem));
    }
}

#[test]
fn missing_null_flag_and_contradictory_entitlement_never_enable_title_fallback() {
    let missing:Product=serde_json::from_value(serde_json::json!({"namespace":"fixture-namespace","title":"Synthetic product","entitlementName":"","eulaIds":[]})).unwrap();
    assert!(!missing.entitlement_name_is_null);assert!(!supported(&missing));
    assert!(!supported(&Product{entitlement_name_is_null:true,..product()}));
    assert!(supported(&product()));assert!(supported(&nullable_product()));
    assert!(!same_product(&missing,&nullable_product()));
}

#[path="epic_redeem_quota_tests.rs"]
mod quota_tests;

#[path="epic_redeem_activation_tests.rs"]
mod activation_tests;

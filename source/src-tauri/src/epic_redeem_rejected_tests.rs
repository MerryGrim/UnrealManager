//! Native-only synthetic fixtures. No Epic, native dialog, account or user file.
use super::*;
use super::super::{input,failure,select_ready_rows,Phase};
const ACCOUNT:&str="0123456789abcdef0123456789abcdef";
const USED:&str="SYNTHETIC-USED-12345";
const OWNED:&str="SYNTHETIC-OWNED-67890";
const READY:&str="SYNTHETIC-READY-24680";
fn draft()->Draft{let mut draft=input(vec![USED.into(),OWNED.into(),READY.into()]).unwrap();draft.account_id=ACCOUNT.into();draft}
fn rejected()->Draft{
    let mut draft=draft();
    for(index,reason,title)in[(0,Reason::AlreadyUsed,"Used fixture product"),(1,Reason::AlreadyOwned,"Owned fixture product")]{
        draft.snapshot.items[index].title=title.into();failure(&mut draft.snapshot.items[index],reason);remember(&mut draft,index,reason,None);
    }draft
}
#[test]
fn only_two_explicit_failed_reasons_are_exportable_and_never_enter_snapshot(){
    let mut draft=rejected();assert_eq!(draft.snapshot.rejected_count,Some(2));
    let encoded=serde_json::to_string(&draft.snapshot).unwrap();for key in[USED,OWNED,READY]{assert!(!encoded.contains(key));}
    assert!(!encoded.contains(ACCOUNT));assert!(encoded.contains("\"rejectedCount\":2"));
    for reason in[Reason::InvalidInput,Reason::Duplicate,Reason::NotFound,Reason::Expired,Reason::RegionRestricted,Reason::AuthRequired,Reason::AccountChanged,Reason::Network,Reason::Service,Reason::RateLimited,Reason::RequiresOfficial,Reason::Unknown,Reason::Cancelled]{
        failure(&mut draft.snapshot.items[2],reason);remember(&mut draft,2,reason,None);
    }
    draft.snapshot.items[2].status=Status::Unknown;remember(&mut draft,2,Reason::AlreadyUsed,None);
    draft.snapshot.items[2].status=Status::Success;remember(&mut draft,2,Reason::AlreadyOwned,None);
    assert_eq!(draft.rejected.lock().unwrap().rows.len(),2);
    let output=format(&draft.rejected.lock().unwrap(),"en");let text=std::str::from_utf8(&output.0).unwrap();
    assert!(text.starts_with('\u{feff}'));assert!(text.contains(USED)&&text.contains(OWNED));assert!(!text.contains(READY));
    assert!(text.contains("This key has already been used.")&&text.contains("The product is already in the account."));
}
#[test]
fn rejected_secrets_survive_ready_subset_and_completed_draft_drop(){
    let mut draft=rejected();draft.snapshot.items[2].status=Status::Ready;
    draft.previews[2]=Some(Product{namespace:"fixture".into(),entitlement_name:"fixture-id".into(),title:"Ready fixture".into(),..Default::default()});
    let shared=draft.rejected.clone();let id=draft.snapshot.batch_id.clone();select_ready_rows(&mut draft);
    assert_eq!(draft.keys.len(),1);assert_eq!(draft.keys[0].as_ref().unwrap().text(),READY);assert_eq!(draft.snapshot.rejected_count,Some(2));
    draft.snapshot.phase=Phase::Completed;drop(draft);
    let mut cache=shared.lock().unwrap();assert!(cache.available(&id,ACCOUNT,Instant::now()));
    let output=format(&cache,"ru");let text=std::str::from_utf8(&output.0).unwrap();assert!(text.contains(USED)&&text.contains(OWNED));assert!(!text.contains(READY));
}
#[test]
fn expiry_account_mismatch_and_explicit_clear_destroy_exportability(){
    for case in 0..3{
        let draft=rejected();let mut cache=draft.rejected.lock().unwrap();let id=draft.snapshot.batch_id.clone();
        match case{0=>{let at=cache.touched+TTL;assert!(!cache.available(&id,ACCOUNT,at));},1=>{assert!(!cache.available(&id,"fedcba9876543210fedcba9876543210",Instant::now()));},_=>{cache.clear();assert!(!cache.available(&id,ACCOUNT,Instant::now()));}}
        assert!(cache.rows.is_empty());assert!(!cache.valid);
    }
}
#[test]
fn progress_refresh_extends_lazy_ttl_without_a_timer_and_different_batch_cannot_export(){
    let draft=rejected();let mut cache=draft.rejected.lock().unwrap();let id=draft.snapshot.batch_id.clone();
    cache.touched=Instant::now()-Duration::from_secs(599);let old_deadline=cache.touched+TTL;
    assert_eq!(cache.touch(),2);assert!(cache.available(&id,ACCOUNT,old_deadline+Duration::from_secs(1)));
    assert!(!cache.available("different-batch",ACCOUNT,Instant::now()));assert_eq!(cache.rows.len(),2,"A foreign batch cannot erase another batch's data");
}
#[test]
fn filename_cancel_never_calls_writer_and_a_write_failure_has_no_success_result(){
    let draft=rejected();let mut cache=draft.rejected.lock().unwrap();let id=draft.snapshot.batch_id.clone();
    let cancelled=save_selected(&mut cache,&id,ACCOUNT,None,"en",|_,_|panic!("Cancel must not create or write any file")).unwrap();
    assert!(!cancelled.saved);assert_eq!(cancelled.count,0);
    assert!(save_selected(&mut cache,&id,ACCOUNT,Some(Path::new("synthetic-only.txt")),"en",|_,_|Err("synthetic write failure".into())).is_err());
    assert_eq!(cache.rows.len(),2,"An export failure leaves known rejected keys available for retry");
    let result=save_selected(&mut cache,&id,ACCOUNT,Some(Path::new("synthetic-only.txt")),"en",|_,bytes|{assert!(bytes.starts_with(&[0xef,0xbb,0xbf]));Ok(())}).unwrap();assert!(result.saved);assert_eq!(result.count,2);
    assert_eq!(serde_json::to_string(&result).unwrap(),"{\"saved\":true,\"count\":2}");
}
#[test]
fn title_cells_are_bounded_escaped_and_missing_title_is_honest_for_all_locales(){
    assert_eq!(cell("A\tB\r\nC\\D\u{0001}"),"A\\tB\\r\\nC\\\\D ");assert_eq!(cell(&"x".repeat(300)).len(),240);
    for locale in["ru","en","de","it","es","fr","zh","ja","ko"]{
        let mut draft=rejected();draft.rejected.lock().unwrap().rows[0].title=String::new();
        let output=format(&draft.rejected.lock().unwrap(),locale);let content=std::str::from_utf8(&output.0).unwrap();assert!(content.contains(text(locale)[3]));assert_eq!(content.lines().count(),3);
        draft.snapshot.items[2].title=format!("Echo {READY}");failure(&mut draft.snapshot.items[2],Reason::AlreadyOwned);remember(&mut draft,2,Reason::AlreadyOwned,None);assert!(draft.rejected.lock().unwrap().rows[2].title.is_empty());
    }
}
struct Fixture(PathBuf);
impl Fixture{fn new()->Self{let parent=std::env::var_os("UM_TEMP_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);crate::vault::no_links(&parent).unwrap();std::fs::create_dir_all(&parent).unwrap();let path=parent.join(format!("um-rejected-export-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&path).unwrap();Self(path)}}
impl Drop for Fixture{fn drop(&mut self){if crate::vault::no_links(&self.0).is_ok(){let _=std::fs::remove_dir_all(&self.0);}}}
#[test]
fn partial_write_failure_preserves_previous_file_and_removes_only_own_temporary(){
    use std::io::Write;
    let fixture=Fixture::new();let path=fixture.0.join("rejected.txt");std::fs::write(&path,b"previous user file").unwrap();
    let error=write_atomic_with(&path,b"replacement fixture",|file,bytes|{file.write_all(&bytes[..3])?;Err(std::io::Error::from(std::io::ErrorKind::WriteZero))}).unwrap_err();
    assert_eq!(error,SAVE_FAILED);assert_eq!(std::fs::read(&path).unwrap(),b"previous user file");assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(),1);
    write_atomic(&path,b"replacement fixture").unwrap();assert_eq!(std::fs::read(&path).unwrap(),b"replacement fixture");assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(),1);
}
#[test]
fn production_slot_operations_for_discard_new_batch_and_shutdown_clear_native_rows(){
    // Exercise the actual pure slot operations used by the native lifecycle,
    // without linking a real Batch/Bridge/WebView drop into this unit fixture.
    let first=rejected();let first_cache=first.rejected.clone();let mut slot=None;replace_slot(&mut slot,first_cache.clone());
    let second=rejected();let second_cache=second.rejected.clone();replace_slot(&mut slot,second_cache.clone());
    assert!(first_cache.lock().unwrap().rows.is_empty());clear_batch(&slot,"unrelated-batch");assert_eq!(second_cache.lock().unwrap().rows.len(),2);
    clear_batch(&slot,&second.snapshot.batch_id);assert!(second_cache.lock().unwrap().rows.is_empty());
    let third=rejected();let third_cache=third.rejected.clone();replace_slot(&mut slot,third_cache.clone());clear_slot(&mut slot);assert!(slot.is_none());assert!(third_cache.lock().unwrap().rows.is_empty());
}

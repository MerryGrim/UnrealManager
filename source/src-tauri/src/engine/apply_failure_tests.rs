//! Opt-in libtest child only: no Tauri, account, Epic record or user engine.
use super::*;
use std::process::{Child,Command,Stdio};

struct OwnedChild(Child);
impl Drop for OwnedChild{fn drop(&mut self){if self.0.try_wait().ok().flatten().is_none(){let _=self.0.kill();let _=self.0.wait();}}}

#[test]
#[ignore = "Only the parent failure test may start this isolated libtest mode"]
fn killed_apply_subprocess_entry(){
    let base=PathBuf::from(std::env::var_os("UM_FAILURE_APPLY_CHILD_ROOT").expect("Owned fixture argument"));
    let parent=PathBuf::from(std::env::var_os("UM_ENGINE_APPLY_FIXTURE_ROOT").expect("Owned fixture parent"));
    assert!(base.is_absolute()&&base.parent()==Some(parent.as_path()));no_links(&base).unwrap();
    let id=fs::read_to_string(base.join("fixture-owner.txt")).unwrap();assert!(uuid::Uuid::parse_str(&id).is_ok());assert_eq!(base.file_name().unwrap(),format!("um-engine-apply-{id}").as_str());
    // The parent owns cleanup even if this child returns/panics unexpectedly.
    let f=std::mem::ManuallyDrop::new(Fixture{base,id,marker_ready:true});
    let (plan,ops):(Plan,Vec<Operation>)=serde_json::from_slice(&fs::read(f.base.join("failure-case.json")).unwrap()).unwrap();
    assert_eq!(Path::new(&plan.info.path),f.root());assert_eq!(ops.len(),2);
    let ready=f.base.join("kill-ready.json");let second=ops[1].name.clone();
    run(&plan,&f.stage(),&ops,settings(1,65536),|s|{
        if s.current_file.as_deref()==Some(second.as_str())&&s.current_file_done.is_some_and(|n|n>0){
            assert_eq!(s.done,1);assert_eq!(hash_file(&f.root().join(&ops[0].name)).unwrap(),*ops[0].new_hash.as_ref().unwrap());
            super::super::super::atomic(&ready,&serde_json::to_vec(&serde_json::json!({"pid":std::process::id(),"completedFiles":s.done,"phase":s.phase,"currentFile":s.current_file,"readBytes":s.read_bytes})).unwrap()).unwrap();
            loop{std::thread::park_timeout(Duration::from_secs(1));}
        }
    }).unwrap();
    panic!("Child reached completion without the required crash barrier");
}

#[test]
fn killed_apply_process_leaves_real_applying_journal_and_resumes_verified_files(){
    let (f,plan,ops)=update_case(2,0,0);
    let parent=f.base.parent().unwrap();
    write_new(&f.base.join("failure-case.json"),&serde_json::to_vec(&(&plan,&ops)).unwrap()).unwrap();
    let stdout=fs::File::create(f.base.join("child.stdout.log")).unwrap();let stderr=fs::File::create(f.base.join("child.stderr.log")).unwrap();
    let mut child=OwnedChild(Command::new(std::env::current_exe().unwrap())
        .args(["--ignored","--exact","engine::install::apply::tests::failure_matrix::killed_apply_subprocess_entry","--nocapture"])
        .env("UM_FAILURE_APPLY_CHILD_ROOT",&f.base).env("UM_ENGINE_APPLY_FIXTURE_ROOT",parent)
        .current_dir(&f.base).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr)).spawn().unwrap());
    let pid=child.0.id();let ready=f.base.join("kill-ready.json");let start=Instant::now();
    while !ready.exists(){
        assert!(start.elapsed()<Duration::from_secs(30),"Owned child did not reach APPLY barrier");
        assert!(child.0.try_wait().unwrap().is_none(),"Owned child exited before APPLY barrier: {}",fs::read_to_string(f.base.join("child.stderr.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(5));
    }
    let observed:serde_json::Value=serde_json::from_slice(&fs::read(&ready).unwrap()).unwrap();assert_eq!(observed["pid"].as_u64(),Some(pid as u64));assert_eq!(observed["completedFiles"].as_u64(),Some(1));
    child.0.kill().unwrap();let exit=child.0.wait().unwrap();assert!(!exit.success());
    let journal:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(journal.status,"applying");assert!(f.root().join(".um-engine-update.json").exists());
    assert_eq!(hash_file(&f.root().join(&ops[0].name)).unwrap(),*ops[0].new_hash.as_ref().unwrap());assert_eq!(hash_file(&f.stage().join("originals").join(&ops[0].name)).unwrap(),*ops[0].old_hash.as_ref().unwrap());
    assert_eq!(hash_file(&f.root().join(&ops[1].name)).unwrap(),*ops[1].old_hash.as_ref().unwrap());assert_eq!(hash_file(&f.stage().join("files").join(&ops[1].name)).unwrap(),*ops[1].new_hash.as_ref().unwrap());
    run(&plan,&f.stage(),&ops,settings(1,65536),|_|{}).unwrap();assert_final(&f,&ops);
    let finished:Journal=serde_json::from_slice(&fs::read(f.stage().join("journal.json")).unwrap()).unwrap();assert_eq!(finished.status,"complete");assert!(!f.root().join(".um-engine-update.json").exists());
    println!("FAILURE_MATRIX_APPLY_KILL {}",serde_json::json!({"childPid":pid,"childExited":true,"abruptExit":exit.code(),"completedBeforeKill":1,"journalAfterKill":"applying","journalAfterResume":"complete","managedFilesVerified":ops.len(),"unrelatedPluginPreserved":true,"scope":"Owned libtest process termination, not power-loss durability"}));
}

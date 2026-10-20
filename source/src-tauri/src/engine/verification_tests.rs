use super::{operations, operations_with_interval};
use crate::engine::{target, Component, FileManifestList, Plan, Preview, install::Operation};
use crate::vault::{hash_file, no_links, relative};
use egs_api::api::types::download_manifest::FileChunkPart;
use std::{collections::BTreeMap, fs, io::Write, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

// Frozen pre-optimization implementation (install.rs:276-348, 2026-10-20).
// Kept test-only: this is the serial behavioral/performance baseline, including
// its original case-sensitive unchanged-manifest shortcut.
fn original_operations(
    plan: &Plan,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<Vec<Operation>, String> {
    let mut last = Instant::now();
    let root = target(&plan.info.path)?;
    let mut operations = Vec::new();
    for (i, (name, f)) in plan.files.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("Проверка остановлена".into());
        }
        if last.elapsed() > Duration::from_millis(250) {
            progress(i as u64, plan.files.len() as u64);
            last = Instant::now();
        }
        let p = root.join(relative(name)?);
        if plan.old.get(name).is_some_and(|old| old.file_hash == f.file_hash) && p.is_file() {
            continue;
        }
        no_links(&p)?;
        let hash = if p.exists() { Some(hash_file(&p)?) } else { None };
        if hash.as_ref().is_some_and(|h| *h == f.file_hash.to_lowercase()) {
            continue;
        }
        if let Some(ref hash) = hash {
            if plan.old.get(name).is_none_or(|o| o.file_hash.to_lowercase() != *hash) {
                return Err(format!("Локальный файл изменён: {name}. Обновление остановлено, файл сохранён."));
            }
        }
        operations.push(Operation { name: name.clone(), old_hash: hash, new_hash: Some(f.file_hash.to_lowercase()) });
    }
    for (name, f) in &plan.old {
        if plan.files.contains_key(name) || (!plan.remove_unselected && plan.tags.contains_key(name)) {
            continue;
        }
        let p = root.join(relative(name)?);
        no_links(&p)?;
        if p.exists() {
            let hash = hash_file(&p)?;
            if hash != f.file_hash.to_lowercase() {
                return Err(format!("Удалённый автором файл изменён локально: {name}"));
            }
            operations.push(Operation { name: name.clone(), old_hash: Some(hash), new_hash: None });
        }
    }
    Ok(operations)
}

struct Fixture { base: PathBuf, id: String, marker_ready: bool }
impl Fixture {
    fn new() -> Self {
        let parent = std::env::var_os("UM_ENGINE_VERIFY_FIXTURE_ROOT")
            .map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute(), "Fixture root must be absolute");
        no_links(&parent).unwrap();
        fs::create_dir_all(&parent).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let base = parent.join(format!("um-engine-verification-{id}"));
        fs::create_dir(&base).unwrap();
        let mut fixture = Self { base, id, marker_ready: false };
        let mut marker = fs::OpenOptions::new().write(true).create_new(true)
            .open(fixture.base.join("fixture-owner.txt")).unwrap();
        marker.write_all(fixture.id.as_bytes()).unwrap();
        fixture.marker_ready = true;
        fs::create_dir(fixture.base.join("engine")).unwrap();
        fixture
    }
    fn root(&self) -> PathBuf { self.base.join("engine") }
    fn write(&self, name: &str, data: &[u8]) {
        let p = self.root().join(relative(name).unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
    fn plan(&self) -> Plan {
        Plan {
            resume_stage: None, catalog_item_id: None, remove_unselected: false,
            source_base: String::new(), raw: Vec::new(), files: BTreeMap::new(),
            old: BTreeMap::new(), item: None, chunk_sizes: BTreeMap::new(), tags: BTreeMap::new(),
            info: Preview {
                id: self.id.clone(), app_name: "UE_SYNTHETIC".into(), build: "synthetic-1".into(),
                path: self.root().to_string_lossy().into_owned(), update: true,
                components: Vec::<Component>::new(), bytes: 0, download_bytes: 0,
                changed: 0, added: 0, removed: 0, required_bytes: 0, free_bytes: 0, conflicts: vec![],
            },
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Failure/panic cleanup is confined to this freshly-created UUID, never
        // to the caller's fixture parent or any engine/user-data location.
        let result = (|| -> Result<(), String> {
            if self.base.file_name().and_then(|n| n.to_str()) != Some(format!("um-engine-verification-{}",self.id).as_str()) {
                return Err("Fixture ownership mismatch".into());
            }
            no_links(&self.base)?;
            if !self.marker_ready {
                // Constructor failed before the complete marker: accept only
                // our newly-created empty directory or its sole partial marker.
                let entries = fs::read_dir(&self.base).map_err(|e|e.to_string())?
                    .collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                if entries.len() == 1 && entries[0].file_name() == "fixture-owner.txt" {
                    let marker = entries[0].path(); no_links(&marker)?;
                    let partial = fs::read(&marker).map_err(|e|e.to_string())?;
                    if !self.id.as_bytes().starts_with(&partial) { return Err("Unknown partial fixture marker".into()); }
                    fs::remove_file(marker).map_err(|e|e.to_string())?;
                } else if !entries.is_empty() { return Err("Unknown incomplete fixture contents".into()); }
                return fs::remove_dir(&self.base).map_err(|e|e.to_string());
            }
            if fs::read(self.base.join("fixture-owner.txt")).map_err(|e|e.to_string())? != self.id.as_bytes() {
                return Err("Fixture marker changed".into());
            }
            let mut pending = vec![self.base.clone()]; let mut count = 0;
            while let Some(dir) = pending.pop() {
                for entry in fs::read_dir(dir).map_err(|e|e.to_string())? {
                    let p = entry.map_err(|e|e.to_string())?.path();
                    let m = fs::symlink_metadata(&p).map_err(|e|e.to_string())?;
                    if m.file_type().is_symlink() { return Err("Linked fixture entry".into()); }
                    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; if m.file_attributes() & 0x400 != 0 { return Err("Reparse fixture entry".into()); } }
                    count += 1; if count > 30_000 { return Err("Fixture inventory bound".into()); }
                    if m.is_dir() { pending.push(p); } else if !m.is_file() { return Err("Unknown fixture entry".into()); }
                }
            }
            fs::remove_dir_all(&self.base).map_err(|e|e.to_string())
        })();
        if let Err(error) = result { eprintln!("Owned verification fixture retained: {error}"); }
    }
}

fn manifest_file(name: &str, data: &[u8]) -> FileManifestList {
    FileManifestList {
        filename: name.into(), file_hash: crate::engine::install::digest(data),
        file_chunk_parts: vec![FileChunkPart { guid: String::new(), link: None, offset: 0, size: data.len() as u128 }],
    }
}
fn encoded(ops: &[Operation]) -> Vec<u8> { serde_json::to_vec(ops).unwrap() }
fn mixed_plan(f: &Fixture) -> Plan {
    let mut p = f.plan();
    for (name, actual, old, new) in [
        ("Engine/same", &b"same"[..], &b"same"[..], &b"same"[..]),
        ("Engine/changed", &b"old"[..], &b"old"[..], &b"new"[..]),
        ("Engine/matching", &b"fresh"[..], &b"prior"[..], &b"fresh"[..]),
    ] {
        f.write(name, actual); p.old.insert(name.into(), manifest_file(name, old));
        p.files.insert(name.into(), manifest_file(name, new));
    }
    p.files.insert("Engine/added".into(), manifest_file("Engine/added", b"addition"));
    f.write("Engine/removed", b"gone");
    p.old.insert("Engine/removed".into(), manifest_file("Engine/removed", b"gone"));
    f.write("Engine/Plugins/UserPlugin/work", b"user work");
    p
}

#[test]
fn parallel_scan_matches_serial_operations_and_reports_all_verified_work() {
    let f = Fixture::new(); let p = mixed_plan(&f); let cancel = AtomicBool::new(false);
    let expected = original_operations(&p, &cancel, |_,_| {}).unwrap();
    assert_eq!(expected.iter().map(|o|o.name.as_str()).collect::<Vec<_>>(), vec!["Engine/added","Engine/changed","Engine/removed"]);
    for workers in [1,2,4] {
        let mut events = vec![];
        let result = operations(&p, &cancel, workers, |s|events.push(s)).unwrap();
        assert_eq!(encoded(&result), encoded(&expected));
        let final_event = events.last().expect("Final progress snapshot");
        assert_eq!((final_event.done,final_event.total,final_event.processed_bytes),(5,5,12));
        assert!(events.windows(2).all(|e|e[0].done<=e[1].done && e[0].processed_bytes<=e[1].processed_bytes));
        assert!(events.iter().all(|e|e.done<=e.total && e.files_per_second.is_finite()));
        assert_eq!(fs::read(f.root().join("Engine/changed")).unwrap(), b"old");
        assert_eq!(fs::read(f.root().join("Engine/Plugins/UserPlugin/work")).unwrap(), b"user work");
        assert!(!f.root().join("Engine/added").exists());
    }
}

#[test]
fn unchanged_manifest_hash_case_does_not_trigger_file_reads() {
    let f=Fixture::new(); f.write("Engine/same", b"same"); let mut p=f.plan();
    let file=manifest_file("Engine/same", b"same"); let mut old=file.clone(); old.file_hash=old.file_hash.to_uppercase();
    p.files.insert(file.filename.clone(),file); p.old.insert(old.filename.clone(),old);
    for workers in [1,2,4] {
        let mut events=vec![];assert!(operations(&p,&AtomicBool::new(false),workers,|s|events.push(s)).unwrap().is_empty());
        let last=events.last().unwrap();assert_eq!((last.done,last.total,last.processed_bytes),(1,1,0));
    }
}

#[test]
fn component_removal_filter_and_missing_paths_match_serial_without_mutation() {
    let f=Fixture::new(); f.write("Engine/optional",b"optional");let mut p=f.plan();
    p.old.insert("Engine/optional".into(),manifest_file("Engine/optional",b"optional"));
    p.old.insert("Engine/missing".into(),manifest_file("Engine/missing",b"absent"));
    p.tags.insert("Engine/optional".into(),vec!["optional".into()]);
    for remove in [false,true] {
        p.remove_unselected=remove;let expected=original_operations(&p,&AtomicBool::new(false),|_,_|{}).unwrap();
        for workers in [1,2,4] {assert_eq!(encoded(&operations(&p,&AtomicBool::new(false),workers,|_|{}).unwrap()),encoded(&expected));}
        assert_eq!(expected.len(),usize::from(remove));
        assert_eq!(fs::read(f.root().join("Engine/optional")).unwrap(),b"optional");
    }
}

#[test]
fn modified_local_file_is_refused_and_user_bytes_remain_unchanged() {
    let f=Fixture::new();f.write("Engine/changed",b"user edit");let mut p=f.plan();
    p.old.insert("Engine/changed".into(),manifest_file("Engine/changed",b"original"));
    p.files.insert("Engine/changed".into(),manifest_file("Engine/changed",b"new version"));
    let expected=original_operations(&p,&AtomicBool::new(false),|_,_|{}).err().expect("Modified original must be refused");
    for workers in [1,2,4] {
        assert_eq!(operations(&p,&AtomicBool::new(false),workers,|_|{}).err().expect("Modified original must be refused"),expected);
        assert_eq!(fs::read(f.root().join("Engine/changed")).unwrap(),b"user edit");
    }
}

#[test]
fn parallel_failure_reports_first_manifest_error_and_preserves_all_files() {
    let f=Fixture::new();let large=vec![37u8;8*1024*1024];let mut p=f.plan();
    f.write("Engine/00-first-error",&large);
    p.old.insert("Engine/00-first-error".into(),manifest_file("Engine/00-first-error",b"original"));
    p.files.insert("Engine/00-first-error".into(),manifest_file("Engine/00-first-error",b"replacement"));
    for name in ["Engine/01-later-error","Engine/02-later-error"] {
        f.write(name,b"other user edit");p.old.insert(name.into(),manifest_file(name,b"original"));
        p.files.insert(name.into(),manifest_file(name,b"replacement"));
    }
    f.write("Engine/03-matching",b"matching");
    p.files.insert("Engine/03-matching".into(),manifest_file("Engine/03-matching",b"matching"));
    p.files.insert("Engine/04-missing".into(),manifest_file("Engine/04-missing",b"addition"));
    let expected=original_operations(&p,&AtomicBool::new(false),|_,_|{}).err().unwrap();
    assert!(expected.contains("Engine/00-first-error"));
    // Five jobs exercise both actual parallel worker counts. The first file
    // requires far more I/O than later failures; correctness must not depend
    // on which worker reports first. No completion-order/timing assertion.
    for workers in [2,4] {
        let error=operations(&p,&AtomicBool::new(false),workers,|_|{}).err().expect("Modified originals must be refused");
        assert_eq!(error,expected);
        assert_eq!(fs::read(f.root().join("Engine/00-first-error")).unwrap(),large);
        for name in ["Engine/01-later-error","Engine/02-later-error"] {assert_eq!(fs::read(f.root().join(name)).unwrap(),b"other user edit");}
        assert_eq!(fs::read(f.root().join("Engine/03-matching")).unwrap(),b"matching");
        assert!(!f.root().join("Engine/04-missing").exists());
    }
}

#[test]
fn cancellation_interrupts_actual_file_reads_before_completing_or_mutating() {
    let f=Fixture::new();let data=vec![19u8;8*1024*1024];f.write("Engine/large",&data);let mut p=f.plan();
    p.files.insert("Engine/large".into(),manifest_file("Engine/large",&data));
    let cancel=AtomicBool::new(false);let mut read=0;let mut cancelled_while_reading=false;
    let result=operations_with_interval(&p,&cancel,1,Duration::ZERO,|s|{
        read=read.max(s.processed_bytes);
        if s.done==0 && s.processed_bytes>0 {cancelled_while_reading=true;cancel.store(true,Ordering::SeqCst);}
    });
    assert!(result.is_err());assert!(cancelled_while_reading);assert!(read>0 && read<data.len()as u64);
    assert_eq!(fs::read(f.root().join("Engine/large")).unwrap(),data);
}

#[test]
fn cancelled_or_escaping_scan_never_produces_installation_operations() {
    let f=Fixture::new();let mut p=f.plan();p.files.insert("Engine/a".into(),manifest_file("Engine/a",b"a"));
    assert!(operations(&p,&AtomicBool::new(true),4,|_|{}).is_err());
    p.files.clear();p.files.insert("../outside".into(),manifest_file("../outside",b"a"));
    assert!(operations(&p,&AtomicBool::new(false),4,|_|{}).is_err());
    assert!(!f.base.join("outside").exists());
}

fn expected_baseline_read_bytes(p:&Plan)->u64 {
    let root=Path::new(&p.info.path);let mut bytes=0;
    for(name,file)in &p.files {
        let path=root.join(name);
        if p.old.get(name).is_some_and(|old|old.file_hash==file.file_hash)&&path.is_file(){continue;}
        if let Ok(m)=fs::metadata(path){bytes+=m.len();}
    }
    for(name,_)in &p.old {
        if p.files.contains_key(name)||(!p.remove_unselected&&p.tags.contains_key(name)){continue;}
        if let Ok(m)=fs::metadata(root.join(name)){bytes+=m.len();}
    }
    bytes
}

#[test]
#[ignore = "Explicit isolated performance observation; no timing-based pass/fail or user-engine reads"]
fn benchmark_serial_baseline_against_bounded_parallel_verification() {
    let f=Fixture::new();let tiny=vec![23u8;4096];let large=vec![29u8;4*1024*1024];let mut current=f.plan();
    for i in 0..4096 {let name=format!("Engine/Tiny/{i:05}.bin");f.write(&name,&tiny);current.files.insert(name.clone(),manifest_file(&name,&tiny));}
    for i in 0..8 {let name=format!("Engine/Large/{i:02}.bin");f.write(&name,&large);current.files.insert(name.clone(),manifest_file(&name,&large));}
    let mut unchanged=current.clone();unchanged.old=unchanged.files.clone();
    let mut mixed=unchanged.clone();mixed.remove_unselected=true;
    for i in 0..32 {let name=format!("Engine/Tiny/{i:05}.bin");if i%2==0{mixed.files.remove(&name);}else{mixed.files.get_mut(&name).unwrap().file_hash=crate::engine::install::digest(b"different target");}}
    for i in 0..16 {let name=format!("Engine/New/{i:02}.bin");mixed.files.insert(name.clone(),manifest_file(&name,b"new"));}
    let production_workers_hint=crate::engine::scan_storage::workers_hint(&f.root());
    for (profile,p) in [("unchanged-metadata",unchanged),("force-hash",current),("mixed",mixed)] {
        let expected_bytes=expected_baseline_read_bytes(&p);let cancel=AtomicBool::new(false);let mut reference=None;
        let expected_total=p.files.len()+p.old.keys().filter(|name|!p.files.contains_key(*name)&&(p.remove_unselected||!p.tags.contains_key(*name))).count();
        for round in 0..2 {
            let modes=if round==0 {vec![0,1,2,4]}else{vec![4,2,1,0]};
            for workers in modes {
                let mut processed=0;let mut final_work=None;let start=Instant::now();
                let ops=if workers==0 {original_operations(&p,&cancel,|_,_|{}).unwrap()}else{operations(&p,&cancel,workers,|s|{processed=s.processed_bytes;final_work=Some((s.done,s.total));}).unwrap()};
                let elapsed=start.elapsed();let encoded=encoded(&ops);if let Some(ref expected)=reference{assert_eq!(&encoded,expected);}else{reference=Some(encoded.clone());}
                if workers!=0 {assert_eq!(processed,expected_bytes);assert_eq!(final_work,Some((expected_total as u64,expected_total as u64)));}
                println!("ENGINE_VERIFY_BENCH {}",serde_json::json!({
                    "schema":1,"profile":profile,"round":round,"algorithm":if workers==0{"original-serial"}else{"bounded-parallel"},"workers":workers.max(1),
                    "fixtureId":&f.id,"manifestFiles":p.files.len(),"fixtureFiles":4104,"fixtureBytes":48*1024*1024,"productionWorkersHint":production_workers_hint,"windowsStorageProbeCompiled":cfg!(windows),
                    "expectedReadBytesFromStableFixture":expected_bytes,"reportedReadBytes":if workers==0{None}else{Some(processed)},
                    "elapsedMs":elapsed.as_secs_f64()*1000.0,"filesPerSecond":expected_total as f64/elapsed.as_secs_f64().max(f64::EPSILON),"expectedReadBytesPerSecond":expected_bytes as f64/elapsed.as_secs_f64().max(f64::EPSILON),
                    "operations":ops.len(),"operationsSha1":crate::engine::install::digest(&encoded),
                    "cacheState":"Uncontrolled OS cache; fixture creation and earlier rounds warm it. No system cache flush.",
                    "fixtureParentFromEnvironment":std::env::var_os("UM_ENGINE_VERIFY_FIXTURE_ROOT").is_some(),"userEngineRead":false
                }));
            }
        }
    }
}

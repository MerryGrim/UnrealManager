//! Opt-in physical disk-full acceptance on a separately prepared tiny NTFS VHD.
//! This module is cfg(test)+Windows only. It never mounts or formats a disk.
use super::*;
use std::{ffi::OsStr,os::windows::ffi::OsStrExt,path::Prefix};
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW,GetDriveTypeW,GetVolumeInformationW,GetVolumePathNameW};

const MIB:u64=1024*1024;
#[derive(Clone)]
struct Volume{root:PathBuf,serial:u32,total:u64,available:u64}
fn wide(value:&OsStr)->Vec<u16>{value.encode_wide().chain(Some(0)).collect()}
fn volume(path:&Path)->Result<Volume,String>{
    no_links(path)?;
    let input=wide(path.as_os_str());let mut mounted=[0u16;512];
    if unsafe{GetVolumePathNameW(input.as_ptr(),mounted.as_mut_ptr(),mounted.len()as u32)}==0{return Err(io(std::io::Error::last_os_error()));}
    let length=mounted.iter().position(|&c|c==0).ok_or("Invalid volume root")?;
    let root=PathBuf::from(String::from_utf16(&mounted[..length]).map_err(|_|"Invalid volume root")?);
    let mount=wide(root.as_os_str());let mut fs_name=[0u16;32];let mut serial=0;
    if unsafe{GetDriveTypeW(mount.as_ptr())}!=3{return Err("A dedicated fixed virtual data disk is required".into());}
    if unsafe{GetVolumeInformationW(mount.as_ptr(),std::ptr::null_mut(),0,&mut serial,std::ptr::null_mut(),std::ptr::null_mut(),fs_name.as_mut_ptr(),fs_name.len()as u32)}==0{return Err(io(std::io::Error::last_os_error()));}
    let fs_len=fs_name.iter().position(|&c|c==0).ok_or("Invalid filesystem name")?;
    if String::from_utf16_lossy(&fs_name[..fs_len])!="NTFS"||serial==0{return Err("A known NTFS volume is required".into());}
    let mut available=0;let mut total=0;
    if unsafe{GetDiskFreeSpaceExW(mount.as_ptr(),&mut available,&mut total,std::ptr::null_mut())}==0{return Err(io(std::io::Error::last_os_error()));}
    if !(16*MIB..=64*MIB).contains(&total){return Err(format!("Refusing volume capacity {total}; only a dedicated 16–64MiB fixture volume is permitted"));}
    Ok(Volume{root,serial,total,available})
}
fn same_volume(expected:&Volume)->Result<Volume,String>{
    let current=volume(&expected.root)?;
    if current.root!=expected.root||current.serial!=expected.serial||current.total!=expected.total{return Err("Fixture volume identity changed".into());}
    Ok(current)
}
struct Fixture{root:PathBuf,volume:Volume,owner:String}
impl Fixture{
    fn clean(&self)->Result<(),String>{
        same_volume(&self.volume)?;no_links(&self.root)?;
        if !self.root.exists(){return Ok(());}
        let entries=fs::read_dir(&self.root).map_err(io)?.collect::<Result<Vec<_>,_>>().map_err(io)?;
        let marker=self.root.join("owner.fixture");no_links(&marker)?;
        let marker_bytes=match fs::read(&marker){
            Ok(bytes)=>bytes,
            Err(e)if e.kind()==std::io::ErrorKind::NotFound&&entries.is_empty()=>return fs::remove_dir(&self.root).map_err(io),
            Err(e)=>return Err(io(e)),
        };
        if marker_bytes!=self.owner.as_bytes(){
            // A failed constructor may leave only its create-new marker prefix.
            if entries.len()==1&&entries[0].file_name()=="owner.fixture"&&entries[0].file_type().map_err(io)?.is_file()&&self.owner.as_bytes().starts_with(&marker_bytes){
                fs::remove_file(&marker).map_err(io)?;return fs::remove_dir(&self.root).map_err(io);
            }
            return Err("Fixture ownership changed; preserved".into());
        }
        for entry in &entries{
            no_links(&entry.path())?;let name=entry.file_name();let name=name.to_string_lossy();
            let own_temp=name.strip_prefix("record.um-").is_some_and(|id|uuid::Uuid::parse_str(id).is_ok_and(|u|u.to_string()==id));
            if !entry.file_type().map_err(io)?.is_file()||!matches!(name.as_ref(),"owner.fixture"|"record.bin"|"filler.bin")&&!own_temp{return Err("Unknown fixture entry; preserved".into());}
        }
        for entry in entries{same_volume(&self.volume)?;no_links(&entry.path())?;fs::remove_file(entry.path()).map_err(io)?;}
        fs::remove_dir(&self.root).map_err(io)
    }
}
impl Drop for Fixture{fn drop(&mut self){let _=self.clean();}}

pub(super) fn run(){
    let root=PathBuf::from(std::env::var_os("UM_REAL_ENOSPC_ROOT").expect("Explicit UM_REAL_ENOSPC_ROOT required; never use the workspace/system volume"));
    let report=PathBuf::from(std::env::var_os("UM_REAL_ENOSPC_REPORT").expect("Explicit UM_REAL_ENOSPC_REPORT outside the full volume required"));
    assert!(root.is_absolute()&&report.is_absolute());
    assert!(matches!(root.components().next(),Some(std::path::Component::Prefix(p))if matches!(p.kind(),Prefix::Disk(_))));
    assert!(matches!(report.components().next(),Some(std::path::Component::Prefix(p))if matches!(p.kind(),Prefix::Disk(_))));
    no_links(&root).unwrap();no_links(&report).unwrap();assert!(!root.exists()&&!report.exists());
    let owner=root.file_name().unwrap().to_str().unwrap().strip_prefix("Case-").expect("Case-UUID leaf required").to_string();
    assert_eq!(uuid::Uuid::parse_str(&owner).unwrap().to_string(),owner);
    let initial=volume(root.parent().unwrap()).expect("Refusing ordinary host filesystem");
    assert_eq!(root.parent().unwrap(),initial.root,"Case must be directly under the dedicated virtual volume root");
    let report_parent=report.parent().unwrap();assert!(report_parent.is_dir());
    let mut report_volume=[0u16;512];assert_ne!(unsafe{GetVolumePathNameW(wide(report_parent.as_os_str()).as_ptr(),report_volume.as_mut_ptr(),report_volume.len()as u32)},0);
    let end=report_volume.iter().position(|&c|c==0).unwrap();let report_volume=PathBuf::from(String::from_utf16(&report_volume[..end]).unwrap());
    assert_ne!(report_volume,initial.root,"Evidence must remain outside the limited filesystem");
    fs::create_dir(&root).unwrap();
    // The only owned subtree is empty until this create-new marker succeeds.
    let fixture=Fixture{root:root.clone(),volume:initial.clone(),owner};
    let marker=root.join("owner.fixture");let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&marker).unwrap();file.write_all(fixture.owner.as_bytes()).unwrap();file.sync_all().unwrap();drop(file);
    let mut raw_code=None;let mut filler_bytes=0u64;
    let outcome=(||->Result<serde_json::Value,String>{
        let record=root.join("record.bin");let filler_path=root.join("filler.bin");let old=b"original synthetic record";
        atomic(&record,old)?;let old_time=fs::metadata(&record).map_err(io)?.modified().map_err(io)?;
        same_volume(&initial)?;
        let mut filler=fs::OpenOptions::new().write(true).create_new(true).open(&filler_path).map_err(io)?;
        let buffer=vec![0x57u8;MIB as usize];
        loop{
            let current=same_volume(&initial)?;
            if current.available<=MIB/2{break;}
            let count=(current.available-MIB/2).min(MIB)as usize;
            if filler_bytes+count as u64>64*MIB{return Err("Bounded filler limit reached; no further write".into());}
            filler.write_all(&buffer[..count]).map_err(io)?;filler.sync_all().map_err(io)?;filler_bytes+=count as u64;
        }
        drop(filler);let before=same_volume(&initial)?;
        if before.available>=8*MIB{return Err("Fixture is not sufficiently full; no expected failure inferred".into());}
        let payload=vec![0x93u8;8*MIB as usize];
        let error=atomic_with_writer(&record,&payload,|file,bytes|{
            // These are genuine WriteFile/flush calls, with no injected error.
            file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|e|{raw_code=e.raw_os_error();io(e)})
        }).err().ok_or("Write unexpectedly fit; real disk-full case not demonstrated")?;
        if !matches!(raw_code,Some(112|39)){return Err(format!("Expected actual raw112/39 from file write/flush, got {raw_code:?}: {error}"));}
        if fs::read(&record).map_err(io)?!=old||fs::metadata(&record).map_err(io)?.modified().map_err(io)?!=old_time{return Err("Original record changed on failed atomic write".into());}
        let entries=fs::read_dir(&root).map_err(io)?.collect::<Result<Vec<_>,_>>().map_err(io)?;
        if entries.len()!=3||entries.iter().any(|e|!matches!(e.file_name().to_str(),Some("owner.fixture"|"record.bin"|"filler.bin"))){return Err("Failed atomic write left a temporary or unknown file".into());}
        same_volume(&initial)?;no_links(&filler_path)?;fs::remove_file(&filler_path).map_err(io)?;
        same_volume(&initial)?;atomic(&record,&payload)?;
        let actual=fs::read(&record).map_err(io)?;let sha1=digest(&actual);
        if actual.len()!=payload.len()||sha1!=digest(&payload){return Err("Retry payload mismatch".into());}
        Ok(serde_json::json!({"actualRawOsError":raw_code,"oldBytesPreserved":true,"oldModificationTimePreserved":true,"failedTemporaryAbsent":true,"retryBytes":actual.len(),"retrySha1":sha1,"fillerBytes":filler_bytes,"availableBeforeAtomic":before.available}))
    })();
    let cleanup=fixture.clean();let absent=!root.exists();
    let value=serde_json::json!({"schema":1,"purpose":"Real native atomic ENOSPC on explicitly supplied bounded NTFS volume only","passed":outcome.is_ok()&&cleanup.is_ok()&&absent,"volumeCapacityBytes":initial.total,"volumeSerial":initial.serial,"root":root,"rawOsError":raw_code,"observations":outcome.as_ref().ok(),"error":outcome.as_ref().err(),"cleanupError":cleanup.as_ref().err(),"fixtureAbsent":absent,"diskCreatedOrMountedByTest":false,"scope":"Atomic metadata-write failure/retry only; not full engine/Vault install or physical power loss"});
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&report).unwrap();file.write_all(&serde_json::to_vec_pretty(&value).unwrap()).unwrap();file.sync_all().unwrap();drop(file);
    assert!(value["passed"].as_bool().unwrap_or(false),"{value}");
}

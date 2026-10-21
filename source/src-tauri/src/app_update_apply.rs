//! One-shot updater entry point. This module never starts Tauri or a WebView.
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, process::{Child, Command}, sync::atomic::{AtomicBool,Ordering}, time::{Duration, Instant}};

pub(crate) const FILES: [&str; 5] = ["unreal-manager.exe", "README.txt", "LICENSE", "THIRD_PARTY_NOTICES.md", "THIRD-PARTY-LICENSES.txt"];
const MAX_COMPRESSED: u64 = 64 * 1024 * 1024;
const MAX_UNPACKED: u64 = 96 * 1024 * 1024;
const MAX_FILE: usize = 64 * 1024 * 1024;
const JOURNAL: &str = ".um-app-update.json";
static UI_ACKNOWLEDGED:AtomicBool=AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Job {
    pub schema: u32, pub id: String, pub parent_pid: u32, pub parent_created: u64,
    pub original_exe: PathBuf, pub mode: String, pub manifest: PathBuf,
    pub signature: PathBuf, pub payload: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle { schema: u32, version: String, files: Vec<BundleFile> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile { name: String, data: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Operation { name: String, old_hash: Option<String>, new_hash: Option<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema: u32, id: String, from_version: String, version: String, mode: String,
    original_exe: PathBuf, job_dir: PathBuf, helper_pid: u32, helper_created: u64,
    helper_sha256: String, app_sha256: String, marker_hash: Option<String>,
    state: String, operations: Vec<Operation>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Healthy { id: String, version: String, app_sha256: String }

fn io(error: impl std::fmt::Display) -> String { format!("App update: {error}") }
fn links(path: &Path) -> Result<(), String> { crate::vault::no_links(path) }
fn canonical(path: &Path) -> Result<PathBuf, String> { links(path)?; fs::canonicalize(path).map_err(io) }
fn same(a: &Path, b: &Path) -> bool {
    #[cfg(windows)] { use std::os::windows::ffi::OsStrExt; a.as_os_str().encode_wide().map(|c| if (65..=90).contains(&c) { c+32 } else { c }).eq(b.as_os_str().encode_wide().map(|c| if (65..=90).contains(&c) { c+32 } else { c })) }
    #[cfg(not(windows))] { a == b }
}
fn valid_id(id: &str) -> bool { uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) }
fn valid_hash(hash: &str) -> bool { hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }
fn hash(path: &Path) -> Result<String, String> { crate::app_updates::hash_file(path) }
fn hash_optional(path: &Path) -> Result<Option<String>, String> {
    links(path)?;
    match fs::metadata(path) {
        Ok(m) if m.is_file() => hash(path).map(Some),
        Ok(_) => Err("App update target is not a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e)),
    }
}
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    links(path)?;
    let mut bytes = Vec::new();
    File::open(path).map_err(io)?.take(limit + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > limit { return Err("App update file exceeds its size limit".into()); }
    Ok(bytes)
}
// Keep authenticated input immutable until the helper has finished executing
// it. Windows share mode denies both write access and rename/deletion; ordinary
// readers (including the installer loader) remain allowed.
fn lock_input(path: &Path) -> Result<File, String> {
    links(path)?;
    let mut options = OpenOptions::new(); options.read(true);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(1); }
    let file = options.open(path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() { return Err("Update input is not a regular file".into()); }
    Ok(file)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    links(path)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(io)?;
    file.write_all(bytes).map_err(io)?; file.sync_all().map_err(io)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    links(path)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    write_new(&temporary, bytes)?;
    let result = fs::rename(&temporary, path).map_err(io);
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}
fn json(path: &Path, value: &impl Serialize) -> Result<(), String> { atomic(path, &serde_json::to_vec_pretty(value).map_err(io)?) }
fn stage(root: &Path, id: &str) -> PathBuf { root.join(format!(".um-app-update-{id}")) }
fn save(j: &Journal) -> Result<(), String> { json(&j.original_exe.parent().ok_or("Missing application folder")?.join(JOURNAL), j) }

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    pub(super) struct Process(pub HANDLE);
    impl Drop for Process { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    impl Process {
        pub fn open(pid: u32) -> Result<Option<Self>, String> {
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid);
                if handle.is_null() {
                    if GetLastError() == ERROR_INVALID_PARAMETER { return Ok(None); }
                    return Err(io(std::io::Error::last_os_error()));
                }
                Ok(Some(Self(handle)))
            }
        }
        pub fn identity(&self) -> Result<(u64, PathBuf), String> {
            unsafe {
                let mut created: FILETIME = std::mem::zeroed();
                let mut exit: FILETIME = std::mem::zeroed();
                let mut kernel: FILETIME = std::mem::zeroed();
                let mut user: FILETIME = std::mem::zeroed();
                if GetProcessTimes(self.0, &mut created, &mut exit, &mut kernel, &mut user) == 0 { return Err(io(std::io::Error::last_os_error())); }
                let mut buffer = vec![0u16; 32768]; let mut size = buffer.len() as u32;
                if QueryFullProcessImageNameW(self.0, 0, buffer.as_mut_ptr(), &mut size) == 0 { return Err(io(std::io::Error::last_os_error())); }
                use std::os::windows::ffi::OsStringExt;
                let image = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size as usize]));
                Ok((((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64, canonical(&image)?))
            }
        }
        pub fn wait(&self, duration: Duration) -> Result<bool, String> {
            match unsafe { WaitForSingleObject(self.0, duration.as_millis().min(u32::MAX as u128 - 1) as u32) } {
                WAIT_OBJECT_0 => Ok(true), WAIT_TIMEOUT => Ok(false), _ => Err(io(std::io::Error::last_os_error())),
            }
        }
    }
    pub(super) struct Mutex(HANDLE);
    impl Drop for Mutex { fn drop(&mut self) { unsafe { ReleaseMutex(self.0); CloseHandle(self.0); } } }
    pub(super) fn lock(root: &Path) -> Result<Option<Mutex>, String> {
        use std::os::windows::ffi::OsStrExt;
        let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().to_ascii_lowercase().as_bytes()));
        let name: Vec<u16> = std::ffi::OsStr::new(&format!("Local\\UnrealManager.AppUpdate.{digest}")).encode_wide().chain(Some(0)).collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() { return Err(io(std::io::Error::last_os_error())); }
            match WaitForSingleObject(handle, 0) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Mutex(handle))),
                WAIT_TIMEOUT => { CloseHandle(handle); Ok(None) },
                _ => { CloseHandle(handle); Err(io(std::io::Error::last_os_error())) },
            }
        }
    }
    struct Handle(HANDLE);
    impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for Attributes { fn drop(&mut self) { unsafe { DeleteProcThreadAttributeList(self.0); } } }
    fn wide(value: &std::ffi::OsStr) -> Vec<u16> { use std::os::windows::ffi::OsStrExt; value.encode_wide().chain(Some(0)).collect() }
    fn quoted(value: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        let mut result=vec![34]; let mut slashes=0;
        for c in value.encode_wide() { if c==92 { slashes+=1; continue; } result.extend(std::iter::repeat_n(92,if c==34 {slashes*2+1}else{slashes})); slashes=0;result.push(c); }
        result.extend(std::iter::repeat_n(92,slashes*2));result.push(34);result
    }
    /// The installer and its descendants join a kill-on-close job atomically
    /// at process creation. They cannot run or escape if the helper crashes.
    pub(super) fn install(payload: &Path, root: &Path, quiescent: &mut bool) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            *quiescent=true;
            let job=Handle(CreateJobObjectW(std::ptr::null(),std::ptr::null()));
            if job.0.is_null() { return Err(io(std::io::Error::last_os_error())); }
            let mut limits:JOBOBJECT_EXTENDED_LIMIT_INFORMATION=std::mem::zeroed();limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(job.0,JobObjectExtendedLimitInformation,&limits as *const _ as _,std::mem::size_of_val(&limits) as u32)==0 { return Err(io(std::io::Error::last_os_error())); }
            let mut size=0;InitializeProcThreadAttributeList(std::ptr::null_mut(),1,0,&mut size);
            if size==0 { return Err(io(std::io::Error::last_os_error())); }
            let mut storage=vec![0usize;size.div_ceil(std::mem::size_of::<usize>())];
            let attributes=storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if InitializeProcThreadAttributeList(attributes,1,0,&mut size)==0 { return Err(io(std::io::Error::last_os_error())); }
            let _attributes=Attributes(attributes);
            if UpdateProcThreadAttribute(attributes,0,PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,&job.0 as *const _ as _,std::mem::size_of::<HANDLE>(),std::ptr::null_mut(),std::ptr::null())==0 { return Err(io(std::io::Error::last_os_error())); }
            let application=wide(payload.as_os_str());let directory=wide(std::ffi::OsStr::new(&installer_directory(root)));
            if root.to_str().is_none() { return Err("Installer target must be a Unicode Windows path".into()); }
            let mut command=quoted(payload.as_os_str());
            for argument in ["/SILENT".to_string(),"/SUPPRESSMSGBOXES".into(),"/NORESTART".into(),"/SP-".into(),format!("/DIR={}",installer_directory(root))] { command.push(32);command.extend(quoted(std::ffi::OsStr::new(&argument))); }command.push(0);
            let mut startup:STARTUPINFOEXW=std::mem::zeroed();startup.StartupInfo.cb=std::mem::size_of::<STARTUPINFOEXW>() as u32;startup.lpAttributeList=attributes;
            let mut information:PROCESS_INFORMATION=std::mem::zeroed();
            *quiescent=false;
            if CreateProcessW(application.as_ptr(),command.as_mut_ptr(),std::ptr::null(),std::ptr::null(),0,EXTENDED_STARTUPINFO_PRESENT|CREATE_SUSPENDED,std::ptr::null(),directory.as_ptr(),&startup.StartupInfo,&mut information)==0 { *quiescent=true;return Err(io(std::io::Error::last_os_error())); }
            let process=Handle(information.hProcess);let thread=Handle(information.hThread);
            let result=(|| {
                if ResumeThread(thread.0)==u32::MAX { return Err(io(std::io::Error::last_os_error())); }
                let start=Instant::now();
                loop {
                    let mut accounting:JOBOBJECT_BASIC_ACCOUNTING_INFORMATION=std::mem::zeroed();
                    if QueryInformationJobObject(job.0,JobObjectBasicAccountingInformation,&mut accounting as *mut _ as _,std::mem::size_of_val(&accounting) as u32,std::ptr::null_mut())==0 { return Err(io(std::io::Error::last_os_error())); }
                    if accounting.ActiveProcesses==0 { *quiescent=true;break; }
                    if start.elapsed()>Duration::from_secs(600) { return Err("Installer process tree exceeded ten minutes".into()); }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let mut code=0;if GetExitCodeProcess(process.0,&mut code)==0 { return Err(io(std::io::Error::last_os_error())); }
                if code==0 { Ok(()) } else { Err(format!("App installer exited with {code}")) }
            })();
            if !*quiescent {
                TerminateJobObject(job.0,1);let start=Instant::now();
                while start.elapsed()<Duration::from_secs(5) {
                    let mut accounting:JOBOBJECT_BASIC_ACCOUNTING_INFORMATION=std::mem::zeroed();
                    if QueryInformationJobObject(job.0,JobObjectBasicAccountingInformation,&mut accounting as *mut _ as _,std::mem::size_of_val(&accounting) as u32,std::ptr::null_mut())!=0 && accounting.ActiveProcesses==0 { *quiescent=true;break; }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            result
        }
    }
}

/// Raw Windows FILETIME creation time and canonical image path, never just PID.
pub fn process_identity(pid: u32) -> Result<(u64, PathBuf), String> {
    #[cfg(windows)] { windows::Process::open(pid)?.ok_or("Update process has exited")?.identity() }
    #[cfg(not(windows))] { let _ = pid; Err("App updates require Windows".into()) }
}
fn helper_alive(j: &Journal) -> Result<bool, String> {
    #[cfg(windows)] {
        let Some(process) = windows::Process::open(j.helper_pid)? else { return Ok(false); };
        if process.wait(Duration::ZERO)? { return Ok(false); }
        let (created, image) = process.identity()?;
        Ok(created == j.helper_created && same(&image, &j.job_dir.join("unreal-manager.exe")) && hash(&image)? == j.helper_sha256)
    }
    #[cfg(not(windows))] { let _ = j; Ok(false) }
}

fn load_job(path: &Path, job_dir: &Path) -> Result<Job, String> {
    load_job_for_recovery(path,job_dir,false)
}
fn load_job_for_recovery(path:&Path,job_dir:&Path,missing_exe:bool)->Result<Job,String>{
    if !same(&canonical(path)?, &job_dir.join("job.json")) { return Err("Update job must be job.json beside its helper".into()); }
    let mut job: Job = serde_json::from_slice(&read_limited(path, 32768)?).map_err(io)?;
    if job.schema != 1 || !valid_id(&job.id) || !matches!(job.mode.as_str(), "portable" | "installed") || job.parent_pid <= 4 || job.parent_created == 0 {
        return Err("Invalid app update job".into());
    }
    for (file, expected) in [(&job.manifest, "manifest.json"), (&job.signature, "manifest.sig")] {
        if !same(&canonical(file)?, &job_dir.join(expected)) { return Err("Update metadata is outside its job directory".into()); }
    }
    job.manifest = canonical(&job.manifest)?;
    job.signature = canonical(&job.signature)?;
    job.payload = canonical(&job.payload)?;
    if job.payload.parent().is_none_or(|p| !same(p, job_dir)) || job.payload.file_name().is_none() { return Err("Update payload is outside its job directory".into()); }
    if job.original_exe.file_name().and_then(|v| v.to_str()) != Some("unreal-manager.exe") { return Err("Invalid application executable name".into()); }
    job.original_exe = if missing_exe {canonical(job.original_exe.parent().ok_or("Missing application folder")?)?.join("unreal-manager.exe")}else{canonical(&job.original_exe)?};
    Ok(job)
}
fn verified(job: &Job) -> Result<crate::app_updates::Manifest, String> {
    let manifest = crate::app_updates::validate_signed_manifest(&read_limited(&job.manifest, 256 * 1024)?, &read_limited(&job.signature, 4096)?)?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(io)?;
    let next = semver::Version::parse(&manifest.version).map_err(io)?;
    if next <= current { return Err("App update is not newer than its trusted helper".into()); }
    let payload = crate::app_updates::payload_for(&manifest, &job.mode)?;
    if payload.size == 0 || payload.size > MAX_COMPRESSED || job.payload.file_name().and_then(|n| n.to_str()) != Some(payload.name.as_str()) || fs::metadata(&job.payload).map_err(io)?.len() != payload.size || hash(&job.payload)? != payload.sha256 {
        return Err("App update payload size, name or SHA-256 mismatch".into());
    }
    Ok(manifest)
}
fn unpack(path: &Path, version: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    if fs::metadata(path).map_err(io)?.len() > MAX_COMPRESSED { return Err("Compressed update exceeds 64 MiB".into()); }
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(File::open(path).map_err(io)?).take(MAX_UNPACKED + 1).read_to_end(&mut decoded).map_err(io)?;
    if decoded.len() as u64 > MAX_UNPACKED { return Err("Unpacked update exceeds 96 MiB".into()); }
    let bundle: Bundle = serde_json::from_slice(&decoded).map_err(io)?;
    decode_bundle(bundle, version)
}
fn decode_bundle(bundle: Bundle, version: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    if bundle.schema != 1 || bundle.version != version || bundle.files.len() != FILES.len() { return Err("Invalid portable update schema, version or file count".into()); }
    let mut seen = BTreeSet::new(); let mut result = Vec::new(); let mut total = 0;
    for file in bundle.files {
        if !FILES.contains(&file.name.as_str()) || !seen.insert(file.name.to_ascii_lowercase()) || file.data.len() > (MAX_FILE + 2) / 3 * 4 { return Err("Portable update contains an invalid or duplicate file".into()); }
        let bytes = base64::engine::general_purpose::STANDARD.decode(file.data).map_err(io)?;
        total += bytes.len();
        if bytes.is_empty() || bytes.len() > MAX_FILE || total as u64 > MAX_UNPACKED { return Err("Portable update file exceeds its size limit".into()); }
        result.push((file.name, bytes));
    }
    if !FILES.iter().all(|name| seen.contains(&name.to_ascii_lowercase())) { return Err("Incomplete portable update".into()); }
    Ok(result)
}

fn validate_journal(j: &Journal, root: &Path) -> Result<(), String> {
    if j.schema != 1 || !valid_id(&j.id) || j.helper_pid <= 4 || j.helper_created == 0 || j.operations.len() != FILES.len() || !same(&j.original_exe, &root.join("unreal-manager.exe")) || !matches!(j.mode.as_str(), "portable" | "installed") || !valid_hash(&j.helper_sha256) || !valid_hash(&j.app_sha256) {
        return Err("Invalid application update recovery journal".into());
    }
    if !matches!(j.state.as_str(), "preparing" | "applying" | "awaitingHealth" | "committed" | "rolledBack" | "needsRecovery") { return Err("Invalid update recovery state".into()); }
    links(&j.job_dir)?; links(&stage(root, &j.id))?;
    let mut seen = BTreeSet::new();
    for operation in &j.operations {
        if !FILES.contains(&operation.name.as_str()) || !seen.insert(&operation.name) || operation.old_hash.iter().chain(operation.new_hash.iter()).any(|h| !valid_hash(h)) { return Err("Invalid update recovery operation".into()); }
        links(&root.join(&operation.name))?; links(&stage(root, &j.id).join("backup").join(&operation.name))?;
    }
    let executable = j.operations.iter().find(|op| op.name == "unreal-manager.exe").ok_or("Missing application recovery operation")?;
    if executable.old_hash.as_deref() != Some(&j.helper_sha256) || (j.mode == "portable" || matches!(j.state.as_str(), "awaitingHealth" | "committed")) && executable.new_hash.as_deref() != Some(&j.app_sha256) { return Err("Application recovery hashes disagree".into()); }
    let old = semver::Version::parse(&j.from_version).map_err(io)?; let new = semver::Version::parse(&j.version).map_err(io)?;
    if new <= old { return Err("Invalid recovery version transition".into()); }
    if hash_optional(&root.join("portable.marker"))? != j.marker_hash || (j.mode == "portable") != j.marker_hash.is_some() { return Err("Portable mode marker changed during update".into()); }
    Ok(())
}
fn load_journal(root: &Path) -> Result<Option<Journal>, String> {
    let path = root.join(JOURNAL); links(&path)?;
    if !path.exists() { return Ok(None); }
    let journal = serde_json::from_slice(&read_limited(&path, 256 * 1024)?).map_err(io)?;
    validate_journal(&journal, root)?; Ok(Some(journal))
}
fn journal_metadata(j: &Journal) -> Result<crate::app_updates::Manifest, String> {
    let job = load_job_for_recovery(&j.job_dir.join("job.json"), &j.job_dir,true)?;
    if job.id != j.id || job.mode != j.mode || !same(&job.original_exe, &j.original_exe) { return Err("Recovery job and journal disagree".into()); }
    // A restarted application has the target version, so validate signatures
    // directly here rather than applying the helper's strictly-newer check.
    let manifest = crate::app_updates::validate_signed_manifest(&read_limited(&job.manifest, 256 * 1024)?, &read_limited(&job.signature, 4096)?)?;
    if manifest.version != j.version || manifest.app_sha256 != j.app_sha256 { return Err("Recovery manifest and journal disagree".into()); }
    Ok(manifest)
}
fn copy_new(source: &Path, destination: &Path) -> Result<(), String> {
    links(source)?; links(destination)?;
    let mut input = File::open(source).map_err(io)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(destination).map_err(io)?;
    std::io::copy(&mut input, &mut output).map_err(io)?; output.sync_all().map_err(io)
}
fn transient_lock(error: &std::io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}
fn replace_file(source: &Path, target: &Path, source_hash: &Option<String>, target_hash: &Option<String>) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if hash_optional(source)? != *source_hash || hash_optional(target)? != *target_hash { return Err("Application file changed before replacement; update stopped".into()); }
        match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(error) if transient_lock(&error) && start.elapsed() < Duration::from_secs(3) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("Replacing {}: {}", target.file_name().unwrap_or_default().to_string_lossy(), io(error))),
        }
    }
}
fn prepare(root: &Path, job_dir: &Path, job: &Job, manifest: &crate::app_updates::Manifest, helper_sha: String, files: &[(String, Vec<u8>)]) -> Result<Journal, String> {
    let stage_root = stage(root, &job.id); links(&stage_root)?;
    fs::create_dir(&stage_root).map_err(io)?;
    fs::create_dir(stage_root.join("new")).map_err(io)?; fs::create_dir(stage_root.join("backup")).map_err(io)?;
    let (created, _) = process_identity(std::process::id())?;
    let mut operations = Vec::new();
    for name in FILES {
        let new_hash = files.iter().find(|(n, _)| n == name).map(|(_, b)| format!("{:x}", Sha256::digest(b)));
        operations.push(Operation { name: name.into(), old_hash: hash_optional(&root.join(name))?, new_hash });
    }
    let j = Journal { schema: 1, id: job.id.clone(), from_version: env!("CARGO_PKG_VERSION").into(), version: manifest.version.clone(), mode: job.mode.clone(), original_exe: root.join("unreal-manager.exe"), job_dir: job_dir.into(), helper_pid: std::process::id(), helper_created: created, helper_sha256: helper_sha, app_sha256: manifest.app_sha256.clone(), marker_hash: hash_optional(&root.join("portable.marker"))?, state: "preparing".into(), operations };
    save(&j)?;
    for operation in &j.operations { if operation.old_hash.is_some() { copy_new(&root.join(&operation.name), &stage_root.join("backup").join(&operation.name))?; } }
    for (name, bytes) in files { write_new(&stage_root.join("new").join(name), bytes)?; }
    Ok(j)
}
fn can_rollback(j: &Journal) -> Result<(), String> {
    let root = j.original_exe.parent().ok_or("Missing application folder")?; validate_journal(j, root)?;
    for operation in &j.operations {
        let current = hash_optional(&root.join(&operation.name))?;
        if current == operation.old_hash { continue; }
        if current.is_some() && current != operation.new_hash { return Err(format!("Update rollback preserved an external edit: {}", operation.name)); }
        if let Some(old) = &operation.old_hash {
            if hash_optional(&stage(root, &j.id).join("backup").join(&operation.name))?.as_ref() != Some(old) { return Err(format!("Update rollback backup is missing or changed: {}", operation.name)); }
        }
    }
    Ok(())
}
fn rollback(j: &Journal) -> Result<(), String> {
    can_rollback(j)?;
    let root = j.original_exe.parent().ok_or("Missing application folder")?;
    for operation in j.operations.iter().rev() {
        let target = root.join(&operation.name); let current = hash_optional(&target)?;
        if current == operation.old_hash { continue; }
        // Recheck immediately before mutation; never guess at an external file.
        if current.is_some() && current != operation.new_hash { return Err(format!("File changed during update rollback: {}", operation.name)); }
        if operation.old_hash.is_some() {
            let source = stage(root, &j.id).join("backup").join(&operation.name);
            let temporary = stage(root, &j.id).join(format!("restore-{}", uuid::Uuid::new_v4()));
            copy_new(&source, &temporary)?; replace_file(&temporary, &target, &operation.old_hash, &current)?;
        } else if current.is_some() { fs::remove_file(target).map_err(io)?; }
    }
    Ok(())
}
fn crash_recovery_allowed(j: &Journal) -> Result<(), String> {
    if j.mode=="installed" && matches!(j.state.as_str(),"applying"|"needsRecovery") && j.operations.iter().any(|op|op.new_hash.is_none()) { return Err("Interrupted installer requires manual recovery. Application files and backups were preserved; automatic rollback was not attempted.".into()); }
    Ok(())
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct RepairFile { name:String,old_hash:Option<String>,current_hash:Option<String> }
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct RepairPreview {schema:u32,id:String,journal_sha256:String,archive:PathBuf,files:Vec<RepairFile>}
fn repair_preview(j:&Journal)->Result<RepairPreview,String>{
    let root=j.original_exe.parent().ok_or("Missing application folder")?;validate_journal(j,root)?;
    if j.mode!="installed" || matches!(j.state.as_str(),"committed"|"rolledBack"){return Err("This journal does not require installed-update repair".into());}
    let mut files=Vec::new();
    for op in &j.operations {
        let backup=stage(root,&j.id).join("backup").join(&op.name);
        for path in [&backup,&root.join(&op.name)]{if path.exists()&&fs::metadata(path).map_err(io)?.len()>MAX_FILE as u64{return Err("Repair file exceeds its size limit; nothing was replaced".into());}}
        if hash_optional(&backup)?!=op.old_hash{return Err(format!("Verified old backup is missing or changed: {}",op.name));}
        files.push(RepairFile{name:op.name.clone(),old_hash:op.old_hash.clone(),current_hash:hash_optional(&root.join(&op.name))?});
    }
    if files.iter().find(|f|f.name=="unreal-manager.exe").and_then(|f|f.old_hash.as_ref())!=Some(&j.helper_sha256){return Err("Old application backup is not the recorded trusted helper".into());}
    Ok(RepairPreview{schema:1,id:j.id.clone(),journal_sha256:hash(&root.join(JOURNAL))?,archive:stage(root,&j.id).join(format!("repair-{}",uuid::Uuid::new_v4())),files})
}
fn repair_preview_matches(j:&Journal,p:&RepairPreview)->Result<(),String>{
    let root=j.original_exe.parent().ok_or("Missing application folder")?;
    if p.schema!=1||p.id!=j.id||p.files.len()!=FILES.len()||p.archive.parent()!=Some(stage(root,&j.id).as_path())||p.archive.file_name().and_then(|n|n.to_str()).and_then(|s|s.strip_prefix("repair-")).is_none_or(|id|!valid_id(id))||hash(&root.join(JOURNAL))?!=p.journal_sha256{return Err("Repair approval or journal changed; nothing was replaced".into());}
    for(op,file)in j.operations.iter().zip(&p.files){if op.name!=file.name||op.old_hash!=file.old_hash||hash_optional(&root.join(&op.name))?!=file.current_hash||hash_optional(&stage(root,&j.id).join("backup").join(&op.name))?!=op.old_hash{return Err(format!("Repair input changed after approval: {}",op.name));}}
    Ok(())
}
// Explicit approval authorizes replacement of exactly FILES, after every
// current byte has a verified retained copy. Unknown data is never discarded.
fn restore_verified_old(j:&Journal,p:&RepairPreview)->Result<PathBuf,String>{
    repair_preview_matches(j,p)?;
    let root=j.original_exe.parent().ok_or("Missing application folder")?;
    let _backups:Vec<File>=j.operations.iter().filter(|op|op.old_hash.is_some()).map(|op|lock_input(&stage(root,&j.id).join("backup").join(&op.name))).collect::<Result<_,_>>()?;
    links(&p.archive)?;fs::create_dir(&p.archive).map_err(io)?;
    json(&p.archive.join("approved-repair.json"),p)?;
    let current_dir=p.archive.join("current");fs::create_dir(&current_dir).map_err(io)?;
    // Keep every retained current copy immutable through all replacements.
    // The old backups alone cannot preserve edits made after the failed update.
    let mut _archived=Vec::new();
    for file in &p.files {if let Some(expected)=&file.current_hash {
        let source=root.join(&file.name);let _guard=lock_input(&source)?;
        if hash(&source)?!=*expected{return Err("Current file changed before repair archive; no application files were replaced".into());}
        let destination=current_dir.join(&file.name);copy_new(&source,&destination)?;
        _archived.push(lock_input(&destination)?);
        if hash(&destination)?!=*expected||hash(&source)?!=*expected{return Err("Repair archive verification failed; application files were not replaced".into());}
    }}
    // Finish and revalidate the entire archive before the first replacement.
    repair_preview_matches(j,p)?;
    for file in &p.files{if hash_optional(&current_dir.join(&file.name))?!=file.current_hash{return Err("Repair archive changed; application files were not replaced".into());}}
    write_new(&p.archive.join("archive-complete"),b"All current owned files were archived and verified before restoration.\n")?;
    for file in &p.files {
        let target=root.join(&file.name);
        if hash_optional(&target)?!=file.current_hash{return Err(format!("Repair stopped on external edit: {}. Archive retained: {}",file.name,p.archive.display()));}
        if file.current_hash==file.old_hash{continue;}
        if file.old_hash.is_some(){
            let source=stage(root,&j.id).join("backup").join(&file.name);let temporary=p.archive.join(format!("restore-{}",uuid::Uuid::new_v4()));copy_new(&source,&temporary)?;
            replace_file(&temporary,&target,&file.old_hash,&file.current_hash)?;
        }else if file.current_hash.is_some(){links(&target)?;fs::remove_file(&target).map_err(io)?;}
    }
    for file in &p.files{if hash_optional(&root.join(&file.name))?!=file.old_hash{return Err("Restored application did not match its verified old backups; all evidence retained".into());}}
    if hash(&root.join(JOURNAL))?!=p.journal_sha256{return Err("Repair journal changed; restored files and evidence were retained".into());}
    json(&p.archive.join("restored.json"),&serde_json::json!({"schema":1,"id":j.id,"restoredOldVersion":j.from_version,"files":p.files,"backupsPreserved":true}))?;
    // Retire, never delete, the exact journal only after restoration proved it
    // is safe to launch the old app. Its backups and all current files remain.
    fs::rename(root.join(JOURNAL),p.archive.join("original-journal.json")).map_err(io)?;
    Ok(p.archive.clone())
}
fn retain_restored_evidence(j: &Journal) -> Result<(), String> {
    let root=j.original_exe.parent().ok_or("Missing application folder")?;
    validate_journal(j,root)?;
    for operation in &j.operations { if hash_optional(&root.join(&operation.name))?!=operation.old_hash { return Err("Restored files changed; recovery journal retained".into()); } }
    let Some(active)=load_journal(root)? else { return Ok(()); };
    if active.id!=j.id || active.helper_sha256!=j.helper_sha256 || active.operations.iter().zip(&j.operations).any(|(a,b)|a.name!=b.name||a.old_hash!=b.old_hash) { return Err("Recovery journal changed; it was preserved in place".into()); }
    let retained=stage(root,&j.id).join(format!("retained-recovery-{}.json",uuid::Uuid::new_v4()));links(&retained)?;
    // Preserve both partial/edited backups and the journal. Only the fixed
    // active journal name is retired after all original files are verified.
    fs::rename(root.join(JOURNAL),retained).map_err(io)
}
fn clean(j: &Journal) -> Result<(), String> {
    let root = j.original_exe.parent().ok_or("Missing application folder")?;
    let owned = stage(root, &j.id); links(&owned)?;
    let mut files = Vec::new(); let mut directories = Vec::new();
    if owned.exists() {
        // Only our fixed flat payload directories are removed. Unexpected files
        // remain available for inspection instead of being recursively deleted.
        for entry in fs::read_dir(&owned).map_err(io)? {
            let entry = entry.map_err(io)?; let name = entry.file_name();
            if name != "new" && name != "backup" { return Err("Unexpected file in application update staging; preserved".into()); }
            links(&entry.path())?;
            if !entry.file_type().map_err(io)?.is_dir() { return Err("Unexpected update staging entry; preserved".into()); }
            for file in fs::read_dir(entry.path()).map_err(io)? {
                let file = file.map_err(io)?; links(&file.path())?;
                if !FILES.iter().any(|name| file.file_name() == *name) || !file.file_type().map_err(io)?.is_file() { return Err("Unexpected update backup entry; preserved".into()); }
                let operation = j.operations.iter().find(|op| file.file_name() == op.name.as_str()).ok_or("Unknown update staging entry")?;
                let expected = if name == "backup" { operation.old_hash.as_ref() } else { operation.new_hash.as_ref() };
                if hash_optional(&file.path())?.as_ref() != expected { return Err("Changed update staging file; backup preserved".into()); }
                files.push((file.path(), expected.cloned()));
            }
            directories.push(entry.path());
        }
    }
    // Complete the preflight before removing any backup. Recheck every file
    // immediately before deletion so later edits cannot be silently discarded.
    for (path, expected) in files { if hash_optional(&path)? != expected { return Err("Update staging changed during cleanup; preserved".into()); } fs::remove_file(path).map_err(io)?; }
    for path in directories { fs::remove_dir(path).map_err(io)?; }
    if owned.exists() { fs::remove_dir(owned).map_err(io)?; }
    let journal = root.join(JOURNAL); links(&journal)?;
    if journal.exists() { fs::remove_file(journal).map_err(io)?; }
    Ok(())
}

trait Running { fn exited(&mut self) -> Result<bool, String>; fn stop(&mut self) -> Result<(), String>; }
impl Running for Child {
    fn exited(&mut self) -> Result<bool, String> { self.try_wait().map(|s| s.is_some()).map_err(io) }
    fn stop(&mut self) -> Result<(), String> { if self.try_wait().map_err(io)?.is_none() { self.kill().map_err(io)?; self.wait().map_err(io)?; } Ok(()) }
}
trait Commands {
    fn install(&mut self, payload: &Path, root: &Path) -> Result<(), String>;
    fn start(&mut self, executable: &Path, restarted: Option<&str>) -> Result<Box<dyn Running>, String>;
    fn installer_quiescent(&self) -> bool { true }
}
#[derive(Default)]
struct Native {
    #[cfg(windows)] lock: Option<windows::Mutex>,
    old_started: bool,
    installer_quiescent: bool,
}
fn installer_directory(root: &Path) -> String {
    let path = root.to_string_lossy();
    // Inno's /DIR parameter is a shell-style Windows path. Internally we retain
    // canonical verbatim paths for identity and containment checks.
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") { format!(r"\\{unc}") }
    else { path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned() }
}
impl Commands for Native {
    fn install(&mut self, payload: &Path, root: &Path) -> Result<(), String> {
        #[cfg(windows)] { windows::install(payload,root,&mut self.installer_quiescent) }
        #[cfg(not(windows))] { let _=(payload,root);Err("App updates require Windows".into()) }
    }
    fn installer_quiescent(&self) -> bool { self.installer_quiescent }
    fn start(&mut self, executable: &Path, restarted: Option<&str>) -> Result<Box<dyn Running>, String> {
        // The restored image must observe an unlocked, cleaned startup. A new
        // image keeps the lock until it has acknowledged native readiness.
        if restarted.is_none() { #[cfg(windows)] { self.lock.take(); } }
        let mut command = Command::new(executable); command.current_dir(installer_directory(executable.parent().ok_or("Missing application folder")?));
        if let Some(id) = restarted { command.arg("--app-update-restarted").arg(id); }
        // Unit-test subprocesses use the test harness instead of the production
        // main entry. This branch does not exist in any distributed EXE.
        #[cfg(test)]
        if restarted.is_none() && std::env::var_os("UM_UPDATE_FIXTURE_DIRECTORY").is_some() {
            command.args(["--ignored", "--exact", "app_update_apply::tests::native_subprocess_entry", "--nocapture"]).env("UM_UPDATE_FIXTURE_MODE", "old");
        }
        let _image = lock_input(executable)?; let expected = hash(executable)?;
        let start = Instant::now();
        let child = loop {
            if hash(executable)? != expected { return Err("Application changed before restart; launch stopped".into()); }
            match command.spawn() {
                Ok(child) => break child,
                Err(error) if transient_lock(&error) && start.elapsed() < Duration::from_secs(3) => std::thread::sleep(Duration::from_millis(50)),
                Err(error) => return Err(format!("Starting application: {}", io(error))),
            }
        };
        if restarted.is_none() { self.old_started = true; }
        Ok(Box::new(child) as Box<dyn Running>)
    }
}
fn wait_healthy(j: &Journal, child: &mut dyn Running, timeout: Duration) -> Result<(), String> {
    let start = Instant::now(); let path = j.job_dir.join("healthy.json");
    while start.elapsed() < timeout {
        if path.exists() {
            let healthy: Healthy = serde_json::from_slice(&read_limited(&path, 4096)?).map_err(io)?;
            if healthy.id == j.id && healthy.version == j.version && healthy.app_sha256 == j.app_sha256 { return Ok(()); }
            return Err("Invalid application ready acknowledgement".into());
        }
        if child.exited()? { return Err("Updated application exited before its UI ready acknowledgement".into()); }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err("Updated application did not acknowledge UI readiness".into())
}
fn portable_apply(j: &Journal) -> Result<(), String> {
    let root = j.original_exe.parent().ok_or("Missing application folder")?;
    for operation in &j.operations {
        let target = root.join(&operation.name); let source = stage(root, &j.id).join("new").join(&operation.name);
        if hash_optional(&target)? != operation.old_hash || hash_optional(&source)? != operation.new_hash { return Err(format!("Update file changed before replacement: {}", operation.name)); }
        replace_file(&source, &target, &operation.new_hash, &operation.old_hash)?;
    }
    Ok(())
}
fn execute(j: &mut Journal, payload: &Path, commands: &mut dyn Commands, health_timeout: Duration) -> Result<(), String> {
    let root = j.original_exe.parent().ok_or("Missing application folder")?.to_path_buf();
    j.state = "applying".into(); save(j)?;
    let applied = if j.mode == "portable" { portable_apply(j) } else {
        let result = commands.install(payload, &root);
        if !commands.installer_quiescent() { j.state="needsRecovery".into();let _=save(j);return Err("Installer process tree did not become quiescent; backup retained without modifying application files".into()); }
        // Installer payloads do not expose individual file hashes. Snapshot its
        // observed result before rollback; later external changes are retained.
        let snapshot = (|| {
            for operation in &mut j.operations { operation.new_hash = hash_optional(&root.join(&operation.name))?; }
            save(j)
        })();
        result.and(snapshot)
    };
    let mut child: Option<Box<dyn Running>> = None;
    let result = applied.and_then(|_| {
        if hash(&j.original_exe)? != j.app_sha256 { return Err("Installed application SHA-256 differs from the signed manifest".into()); }
        j.state = "awaitingHealth".into(); save(j)?;
        child = Some(commands.start(&j.original_exe, Some(&j.id))?);
        wait_healthy(j, child.as_mut().unwrap().as_mut(), health_timeout)
    });
    if let Err(error) = result {
        if let Some(child) = child.as_mut() { if let Err(stop) = child.stop() {
            j.state = "needsRecovery".into(); let _ = save(j);
            return Err(format!("{error}. Owned replacement process could not be stopped; backup retained: {stop}"));
        } }
        restore_after_failure(j, commands, &error)
    } else { j.state = "committed".into(); save(j)?; clean(j) }
}
fn restore_after_failure(j: &mut Journal, commands: &mut dyn Commands, error: &str) -> Result<(), String> {
    match rollback(j) {
        Ok(()) => {
            j.state = "rolledBack".into();
            let cleanup = save(j).and_then(|_| clean(j));
            let safe_start = cleanup.is_ok() || retain_restored_evidence(j).is_ok();
            let restarted = if safe_start { commands.start(&j.original_exe, None) } else { Err("Recovery evidence needs review before restart".into()) };
            let detail = match (cleanup, restarted) {
                (Ok(()), Ok(_)) => String::new(),
                (Err(e), Ok(_)) => format!(" Recovery evidence retained: {e}"),
                (Ok(()), Err(e)) => format!(" Restart the restored application manually: {e}"),
                (Err(a), Err(b)) => format!(" Recovery evidence retained: {a}. Restart manually: {b}"),
            };
            Err(format!("{error}. Previous application files were restored.{detail}"))
        }
        Err(rollback) => { j.state = "needsRecovery".into(); let _ = save(j); Err(format!("{error}. Rollback retained its backup: {rollback}")) },
    }
}

fn run_inner(job_path: &Path) -> Result<(), String> {
    let helper = canonical(&std::env::current_exe().map_err(io)?)?;
    if helper.file_name().and_then(|n| n.to_str()) != Some("unreal-manager.exe") { return Err("Invalid temporary helper executable name".into()); }
    let job_dir = helper.parent().ok_or("Missing helper folder")?;
    let _job_file = lock_input(job_path)?;
    let job = load_job(job_path, job_dir)?;
    if same(&helper, &job.original_exe) || job.parent_pid == std::process::id() { return Err("The updater must be a separate temporary copy".into()); }
    let _inputs = [lock_input(&job.manifest)?, lock_input(&job.signature)?, lock_input(&job.payload)?];
    let manifest = verified(&job)?;
    let original = canonical(&job.original_exe)?; let root = original.parent().ok_or("Missing application folder")?;
    #[cfg(windows)] {
        let parent = windows::Process::open(job.parent_pid)?.ok_or("Original application exited before helper attachment")?;
        let (created, image) = parent.identity()?;
        if created != job.parent_created || !same(&image, &original) { return Err("Original application process identity changed".into()); }
        let lock = windows::lock(root)?.ok_or("Another app update helper is active")?;
        let existing = load_journal(root)?; let helper_sha = hash(&helper)?;
        let original_sha = hash(&original)?;
        if let Some(j) = existing.as_ref() {
            journal_metadata(j)?;
            if j.id != job.id || j.helper_sha256 != helper_sha || j.from_version != env!("CARGO_PKG_VERSION") || (original_sha != helper_sha && original_sha != j.app_sha256) || helper_alive(j)? { return Err("Recovery helper or original application does not match its journal".into()); }
            crash_recovery_allowed(j)?;can_rollback(j)?;
        } else if original_sha != helper_sha { return Err("Temporary helper is not an exact copy of the original application".into()); }
        if job.mode == "portable" && !root.join("portable.marker").is_file() || job.mode == "installed" && root.join("portable.marker").exists() { return Err("Application installation mode changed".into()); }
        let files = if job.mode == "portable" { unpack(&job.payload, &manifest.version)? } else { Vec::new() };
        if job.mode == "portable" && files.iter().find(|(name,_)| name == "unreal-manager.exe").map(|(_,bytes)| format!("{:x}", Sha256::digest(bytes))).as_deref() != Some(manifest.app_sha256.as_str()) { return Err("Portable application does not match the signed application hash".into()); }
        let healthy = job_dir.join("healthy.json"); links(&healthy)?; if healthy.exists() { fs::remove_file(&healthy).map_err(io)?; }
        json(&job_dir.join("ready.json"), &serde_json::json!({"id":job.id,"helperPid":std::process::id()}))?;
        if !parent.wait(Duration::from_secs(120))? { return Err("Original application did not exit within the update timeout".into()); }
        drop(parent);
        let mut commands = Native { lock: Some(lock), old_started: false, installer_quiescent: true };
        let result = (|| {
        // Recheck authenticated data after the original process has exited.
        let rechecked = verified(&job)?;
        if rechecked.version != manifest.version || rechecked.app_sha256 != manifest.app_sha256 { return Err("Authenticated update metadata changed while waiting".into()); }
        if let Some(mut j) = existing {
            j.helper_pid = std::process::id(); j.helper_created = process_identity(std::process::id())?.0; save(&j)?;
            rollback(&j)?; j.state = "rolledBack".into();
            let cleanup=save(&j).and_then(|_|clean(&j));
            if cleanup.is_err() { retain_restored_evidence(&j)?; }
            commands.start(&j.original_exe, None)?;
            return cleanup;
        }
        if hash(&original)? != helper_sha { return Err("Original application changed after process exit; update was not applied".into()); }
        let mut j = match prepare(root, job_dir, &job, &manifest, helper_sha.clone(), &files) {
            Ok(journal) => journal,
            Err(error) => {
                if let Some(mut journal) = load_journal(root)? {
                    if journal.id != job.id { return Err("Unexpected recovery journal; existing backup preserved".into()); }
                    return restore_after_failure(&mut journal, &mut commands, &error);
                }
                // Preparation never changes installed files. Even when no
                // journal could be written, restart only the original image.
                if hash(&original)? == helper_sha { commands.start(&original, None)?; }
                return Err(error);
            }
        };
        execute(&mut j, &job.payload, &mut commands, Duration::from_secs(20))
        })();
        if let Err(error) = result.as_ref() {
            // All failures after original-process exit either restore and
            // restart the exact old image, or retain evidence and explain the
            // stopped update without constructing any WebView.
            if !commands.old_started {
                if load_journal(root).is_ok_and(|j| j.is_none()) && hash(&original).is_ok_and(|h| h == helper_sha) {
                    let _ = commands.start(&original, None);
                }
                #[cfg(not(test))]
                if !commands.old_started { crate::report_app_update_error(error); }
                #[cfg(test)] let _ = error;
            }
        }
        result
    }
    #[cfg(not(windows))] { let _ = (manifest, root); Err("App updates require Windows".into()) }
}

pub fn run(job_path: &Path) -> Result<(), String> {
    let result = run_inner(job_path);
    // Error reporting is confined to the actual temporary helper directory.
    if let Ok(helper) = std::env::current_exe().map_err(io).and_then(|p| canonical(&p)) {
        if let Some(dir) = helper.parent() {
            if canonical(job_path).is_ok_and(|path| same(&path, &dir.join("job.json"))) {
                let _ = json(&dir.join("result.json"), &serde_json::json!({"ok":result.is_ok(),"error":result.as_ref().err()}));
            }
        }
    }
    result
}
pub fn restart_id() -> Option<String> {
    let args: Vec<_> = std::env::args_os().collect();
    args.windows(2).find(|pair| pair[0] == "--app-update-restarted").and_then(|pair| pair[1].to_str()).filter(|id| valid_id(id)).map(str::to_owned)
}
fn validate_restart(j: &Journal, id: &str) -> Result<(), String> {
    let manifest = journal_metadata(j)?;
    if j.id != id || j.state != "awaitingHealth" || manifest.version != env!("CARGO_PKG_VERSION") || hash(&j.original_exe)? != manifest.app_sha256 || !helper_alive(j)? { return Err("Invalid application update restart acknowledgement".into()); }
    Ok(())
}
pub fn confirm_ready(id: &str) -> Result<(), String> {
    let executable = canonical(&std::env::current_exe().map_err(io)?)?;
    let root = executable.parent().ok_or("Missing application folder")?;
    let journal = load_journal(root)?.ok_or("Missing application update journal")?;
    validate_restart(&journal, id)?;
    json(&journal.job_dir.join("healthy.json"), &Healthy { id: id.into(), version: journal.version, app_sha256: journal.app_sha256 })
}
fn acknowledge_ui_once(flag:&AtomicBool,confirm:impl FnOnce()->Result<(),String>)->Result<bool,String>{
    if flag.swap(true,Ordering::SeqCst){return Ok(false);}
    if let Err(error)=confirm(){flag.store(false,Ordering::SeqCst);return Err(error);}
    Ok(true)
}
#[tauri::command]
pub async fn app_update_ui_ready()->Result<bool,String>{
    let Some(id)=restart_id() else{return Ok(false)};
    tauri::async_runtime::spawn_blocking(move||acknowledge_ui_once(&UI_ACKNOWLEDGED,||confirm_ready(&id))).await.map_err(io)?
}
struct RepairArguments{journal:PathBuf,no_restart:bool,parent:Option<(u32,u64)>,installer:Option<PathBuf>}
fn repair_arguments(args:Vec<std::ffi::OsString>)->Result<RepairArguments,String>{
    let mut args=args.into_iter();let journal=args.next().map(PathBuf::from).ok_or("Missing installed update journal")?;
    let mut result=RepairArguments{journal,no_restart:false,parent:None,installer:None};
    while let Some(flag)=args.next(){match flag.to_str(){
        Some("--no-restart")if !result.no_restart=>result.no_restart=true,
        Some("--installer")if result.installer.is_none()=>result.installer=Some(args.next().map(PathBuf::from).ok_or("Missing installer path")?),
        Some("--wait-parent")if result.parent.is_none()=>{let pid=args.next().and_then(|v|v.to_str().and_then(|s|s.parse::<u32>().ok())).filter(|id|*id>4).ok_or("Invalid repair parent")?;let created=args.next().and_then(|v|v.to_str().and_then(|s|s.parse::<u64>().ok())).filter(|v|*v>0).ok_or("Invalid repair parent creation time")?;result.parent=Some((pid,created));},
        _=>return Err("Invalid repair arguments".into()),
    }}
    if result.installer.is_some()&&!result.no_restart{return Err("Installer recovery must not restart the app".into());}Ok(result)
}
fn repair_context(path:&Path)->Result<(Journal,Job),String>{
    if path.file_name().and_then(|s|s.to_str())!=Some(JOURNAL){return Err("Select the installed application's .um-app-update.json journal".into());}
    let root=canonical(path.parent().ok_or("Missing repair folder")?)?;
    let j=load_journal(&root)?.ok_or("No interrupted update journal was found")?;
    if j.mode!="installed"{return Err("Explicit installer repair supports the installed edition only".into());}
    let manifest=journal_metadata(&j)?;let job=load_job_for_recovery(&j.job_dir.join("job.json"),&j.job_dir,true)?;
    let payload=crate::app_updates::payload_for(&manifest,"installed")?;
    if job.payload.file_name().and_then(|n|n.to_str())!=Some(payload.name.as_str())||fs::metadata(&job.payload).map_err(io)?.len()!=payload.size||hash(&job.payload)?!=payload.sha256{return Err("Signed installer payload changed; repair stopped".into());}
    if hash(&j.job_dir.join("unreal-manager.exe"))?!=j.helper_sha256{return Err("Original updater copy changed; repair stopped".into());}
    Ok((j,job))
}
fn installer_claim_matches(actual:&Path,expected:&Path,actual_hash:&str,expected_hash:&str,live:bool,ancestor:bool)->bool{
    live&&ancestor&&same(actual,expected)&&actual_hash==expected_hash
}
#[cfg(windows)]
fn descendant_of(pid:u32,ancestor:u32)->Result<bool,String>{
    use windows_sys::Win32::{Foundation::*,System::Diagnostics::ToolHelp::*};
    unsafe{let h=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0);if h==INVALID_HANDLE_VALUE{return Err(io(std::io::Error::last_os_error()));}
        struct Snapshot(HANDLE);impl Drop for Snapshot{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}let _guard=Snapshot(h);
        let mut item:PROCESSENTRY32W=std::mem::zeroed();item.dwSize=std::mem::size_of::<PROCESSENTRY32W>()as u32;let mut parents=std::collections::BTreeMap::new();
        let mut more=Process32FirstW(h,&mut item);while more!=0{parents.insert(item.th32ProcessID,item.th32ParentProcessID);more=Process32NextW(h,&mut item);}
        let mut at=pid;let mut seen=BTreeSet::new();for _ in 0..64{if !seen.insert(at){break;}let Some(parent)=parents.get(&at).copied()else{break};if parent==ancestor{return Ok(true);}at=parent;}Ok(false)
    }
}
fn active_installer(j:&Journal,job:&Job,installer:&Path)->Result<bool,String>{
    #[cfg(windows)]{let manifest=journal_metadata(j)?;let expected=crate::app_updates::payload_for(&manifest,"installed")?;
        Ok(installer_claim_matches(&canonical(installer)?,&job.payload,&hash(installer)?,&expected.sha256,j.state=="applying"&&helper_alive(j)?,descendant_of(std::process::id(),j.helper_pid)?))
    }
    #[cfg(not(windows))]{let _=(j,job,installer);Ok(false)}
}
fn approve_repair(j:&Journal,p:&RepairPreview)->Result<bool,String>{
    #[cfg(windows)]unsafe{
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let unknown=p.files.iter().filter(|f|f.current_hash!=f.old_hash).count();
        let text=format!("Interrupted Unreal Manager update / Прервано обновление Unreal Manager\n\nRestore verified version {}? / Восстановить проверенную версию {}?\n\nExactly five application files are affected; {unknown} differ. Current files and all old backups will be retained here:\n{}\n\nData, Epic sign-in, downloads, projects and other files are not changed. No administrator restart is requested.\n\nБудут сохранены текущие файлы и резервные копии. Данные, вход в Epic, загрузки и проекты не изменяются.\n\nContinue / Продолжить?",j.from_version,j.from_version,p.archive.display());
        let wide:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();let title:Vec<u16>="Unreal Manager — repair / восстановление".encode_utf16().chain(Some(0)).collect();
        Ok(MessageBoxW(std::ptr::null_mut(),wide.as_ptr(),title.as_ptr(),MB_YESNO|MB_ICONWARNING|MB_DEFBUTTON2)==IDYES)
    }
    #[cfg(not(windows))]{let _=(j,p);Err("Application repair requires Windows".into())}
}
pub fn repair(args:Vec<std::ffi::OsString>)->Result<bool,String>{
    let arguments=repair_arguments(args)?;
    let(j,job)=repair_context(&arguments.journal)?;
    if let Some(installer)=arguments.installer.as_ref(){if active_installer(&j,&job,installer)?{return Ok(true);}}
    if helper_alive(&j)?{return Err("Another application updater is active; repair did not change files".into());}
    if crate::elevation::is_elevated(){return Err("Run application repair as the original Windows user, without administrator elevation".into());}
    #[cfg(windows)]{
        if let Some((pid,created))=arguments.parent{
            let parent=windows::Process::open(pid)?.ok_or("Repair parent exited before attachment")?;let(actual,image)=parent.identity()?;
            if actual!=created||!same(&image,&j.original_exe){return Err("Repair parent identity changed".into());}
            let exe=canonical(&std::env::current_exe().map_err(io)?)?;let runner=exe.parent().ok_or("Repair runner folder")?;
            if runner.parent()!=Some(stage(j.original_exe.parent().unwrap(),&j.id).as_path())||runner.file_name().and_then(|n|n.to_str()).and_then(|s|s.strip_prefix("repair-runner-")).is_none_or(|id|!valid_id(id)){return Err("Unexpected repair runner folder".into());}
            repair_preview(&j)?;
            json(&runner.join("repair-ready.json"),&serde_json::json!({"id":j.id,"helperPid":std::process::id()}))?;
            if !parent.wait(Duration::from_secs(120))?{return Err("Close the original application before repair".into());}
        }
        let root=j.original_exe.parent().unwrap();let lock=windows::lock(root)?.ok_or("Another application update or repair is active")?;
        let(j,job)=repair_context(&arguments.journal)?;
        let _inputs=[lock_input(&j.job_dir.join("job.json"))?,lock_input(&job.manifest)?,lock_input(&job.signature)?,lock_input(&job.payload)?,lock_input(&j.job_dir.join("unreal-manager.exe"))?];
        repair_context(&arguments.journal)?;
        let preview=repair_preview(&j)?;
        if !approve_repair(&j,&preview)?{return Ok(false);}
        let archive=restore_verified_old(&j,&preview).map_err(|e|format!("{e}. Repair evidence retained: {}",preview.archive.display()))?;
        let mut commands=Native{lock:Some(lock),old_started:false,installer_quiescent:true};
        if !arguments.no_restart{commands.start(&j.original_exe,None).map_err(|e|format!("Old files restored. Start the application manually. Archive: {}. {e}",archive.display()))?;}
        Ok(true)
    }
    #[cfg(not(windows))]{let _=(j,job);Err("Application repair requires Windows".into())}
}
fn start_repair_runner(j:&Journal)->Result<bool,String>{
    // The opt-in native subprocess fixture is a Rust test executable, not the
    // production CLI. Prove signed repair availability without opening a
    // native confirmation dialog or pretending that executable parses CLI.
    #[cfg(test)]{let root=j.original_exe.parent().ok_or("Missing application folder")?;repair_context(&root.join(JOURNAL))?;repair_preview(j)?;Err("Interrupted installer requires explicit repair; verified old backups are available".into())}
    #[cfg(all(windows,not(test)))]{
        let root=j.original_exe.parent().ok_or("Missing application folder")?;
        let _lock=windows::lock(root)?.ok_or("Another application update or repair is active")?;
        repair_context(&root.join(JOURNAL))?;repair_preview(j)?;
        let folder=stage(root,&j.id).join(format!("repair-runner-{}",uuid::Uuid::new_v4()));links(&folder)?;fs::create_dir(&folder).map_err(io)?;
        let original=canonical(&std::env::current_exe().map_err(io)?)?;let _image=lock_input(&original)?;let expected=hash(&original)?;
        let runner=folder.join("unreal-manager.exe");copy_new(&original,&runner)?;if hash(&runner)?!=expected{return Err("Repair runner copy changed".into());}
        let created=process_identity(std::process::id())?.0;
        let mut child=Command::new(&runner).arg("--repair-app-update").arg(root.join(JOURNAL)).arg("--wait-parent").arg(std::process::id().to_string()).arg(created.to_string()).spawn().map_err(io)?;
        let result=(||{let start=Instant::now();while start.elapsed()<Duration::from_secs(10){if child.try_wait().map_err(io)?.is_some(){return Err("Repair runner exited before attachment".into());}let path=folder.join("repair-ready.json");if path.exists(){let ready:serde_json::Value=serde_json::from_slice(&read_limited(&path,4096)?).map_err(io)?;if ready["id"].as_str()!=Some(j.id.as_str())||ready["helperPid"].as_u64()!=Some(child.id()as u64){return Err("Invalid repair runner acknowledgement".into());}let(_,image)=process_identity(child.id())?;if !same(&image,&canonical(&runner)?)||hash(&runner)?!=expected{return Err("Repair runner identity changed".into());}return Ok(true);}std::thread::sleep(Duration::from_millis(25));}Err("Repair runner did not attach; files were not changed".into())})();
        if result.is_err(){child.stop()?;}result
    }
    #[cfg(all(not(windows),not(test)))]{let _=j;Err("Application repair requires Windows".into())}
}
/// Called before constructing Tauri. True means a one-shot helper owns startup.
pub fn recover_or_busy() -> Result<bool, String> {
    let executable = canonical(&std::env::current_exe().map_err(io)?)?;
    let root = executable.parent().ok_or("Missing application folder")?;
    let existing = load_journal(root)?;
    if let Some(mut j) = existing {
        journal_metadata(&j)?;
        if helper_alive(&j)? {
            if let Some(id) = restart_id() { if validate_restart(&j, &id).is_ok() { return Ok(false); } }
            return Ok(true);
        }
        if matches!(j.state.as_str(), "committed" | "rolledBack") {
            let expected = if j.state == "committed" { &j.app_sha256 } else { &j.helper_sha256 };
            let expected_version = if j.state == "committed" { &j.version } else { &j.from_version };
            if hash(&executable)? != *expected || env!("CARGO_PKG_VERSION") != expected_version { return Err("Completed update recovery does not match this application; backup retained".into()); }
            if let Err(error)=clean(&j) { if j.state!="rolledBack" || retain_restored_evidence(&j).is_err() { return Err(error); } } return Ok(false);
        }
        if crash_recovery_allowed(&j).is_err(){return start_repair_runner(&j);}
        can_rollback(&j)?;
        let helper = canonical(&j.job_dir.join("unreal-manager.exe"))?;
        if hash(&helper)? != j.helper_sha256 { return Err("Recovery helper changed; update backup preserved".into()); }
        let mut job = load_job(&j.job_dir.join("job.json"), &j.job_dir)?;
        job.parent_pid = std::process::id(); job.parent_created = process_identity(std::process::id())?.0;
        json(&j.job_dir.join("job.json"), &job)?;
        let ready = j.job_dir.join("ready.json"); links(&ready)?; if ready.exists() { fs::remove_file(&ready).map_err(io)?; }
        let mut child = Command::new(&helper).arg("--apply-app-update").arg(j.job_dir.join("job.json")).spawn().map_err(io)?;
        let handoff: Result<bool, String> = (|| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                if child.try_wait().map_err(io)?.is_some() { return Err("Recovery helper exited before acknowledging startup; backup preserved".into()); }
                if ready.exists() {
                    let acknowledgement: serde_json::Value = serde_json::from_slice(&read_limited(&ready, 4096)?).map_err(io)?;
                    if acknowledgement["id"].as_str() != Some(j.id.as_str()) || acknowledgement["helperPid"].as_u64() != Some(child.id() as u64) { return Err("Recovery acknowledgement did not match the owned helper".into()); }
                    let (created, image) = process_identity(child.id())?;
                    if !same(&image, &helper) { return Err("Recovery helper process image changed".into()); }
                    j.helper_pid = child.id(); j.helper_created = created; save(&j)?; return Ok(true);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err("Recovery helper did not acknowledge startup; backup preserved".into())
        })();
        if handoff.is_err() { if let Err(stop) = child.stop() { return Err(format!("{}. Owned recovery helper could not be stopped: {stop}", handoff.unwrap_err())); } }
        return handoff;
    }
    #[cfg(windows)] { Ok(windows::lock(root)?.is_none()) }
    #[cfg(not(windows))] { Ok(false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture { root: PathBuf, job: PathBuf, journal: Journal }
    impl Fixture {
        fn new(mode: &str) -> Self {
            let base = std::env::temp_dir().join(format!("um-app-update-test-{}", uuid::Uuid::new_v4()));
            let root = base.join("app"); let job = base.join("job"); fs::create_dir_all(&root).unwrap(); fs::create_dir(&job).unwrap();
            fs::create_dir_all(root.join("Data/Account")).unwrap(); fs::write(root.join("Data/Account/session.fixture"), b"preserve-account").unwrap();
            fs::write(root.join("custom-user-file"), b"preserve-custom").unwrap();
            if mode == "portable" { fs::write(root.join("portable.marker"), b"preserve-marker").unwrap(); }
            let id = uuid::Uuid::new_v4().to_string(); let owned = stage(&root,&id); fs::create_dir(&owned).unwrap();fs::create_dir(owned.join("new")).unwrap();fs::create_dir(owned.join("backup")).unwrap();
            let mut operations=Vec::new();
            for name in FILES {
                fs::write(root.join(name),format!("old-{name}")).unwrap();fs::write(owned.join("backup").join(name),format!("old-{name}")).unwrap();fs::write(owned.join("new").join(name),format!("new-{name}")).unwrap();
                operations.push(Operation{name:name.into(),old_hash:hash_optional(&root.join(name)).unwrap(),new_hash:hash_optional(&owned.join("new").join(name)).unwrap()});
            }
            let journal=Journal{schema:1,id,from_version:"0.1.0".into(),version:"0.2.0".into(),mode:mode.into(),original_exe:root.join("unreal-manager.exe"),job_dir:job.clone(),helper_pid:12345,helper_created:1,helper_sha256:hash(&root.join("unreal-manager.exe")).unwrap(),app_sha256:hash(&owned.join("new/unreal-manager.exe")).unwrap(),marker_hash:hash_optional(&root.join("portable.marker")).unwrap(),state:"preparing".into(),operations};
            save(&journal).unwrap(); Self{root,job,journal}
        }
        fn preserved(&self){assert_eq!(fs::read(self.root.join("Data/Account/session.fixture")).unwrap(),b"preserve-account");assert_eq!(fs::read(self.root.join("custom-user-file")).unwrap(),b"preserve-custom");if self.journal.mode=="portable"{assert_eq!(fs::read(self.root.join("portable.marker")).unwrap(),b"preserve-marker");}}
        fn old(&self){for name in FILES{assert_eq!(fs::read(self.root.join(name)).unwrap(),format!("old-{name}").as_bytes());}self.preserved();}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(self.root.parent().unwrap());}}
    struct FakeRunning { stopped: std::rc::Rc<std::cell::Cell<bool>> }
    impl Running for FakeRunning {fn exited(&mut self)->Result<bool,String>{Ok(false)}fn stop(&mut self)->Result<(),String>{self.stopped.set(true);Ok(())}}
    struct FakeCommands { healthy:bool, install_failure:bool, starts:usize, root:PathBuf, job:PathBuf, version:String, sha:String, stopped:std::rc::Rc<std::cell::Cell<bool>> }
    impl FakeCommands {fn for_fixture(f:&Fixture,healthy:bool)->Self{Self{healthy,install_failure:false,starts:0,root:f.root.clone(),job:f.job.clone(),version:f.journal.version.clone(),sha:f.journal.app_sha256.clone(),stopped:Default::default()}}}
    impl Commands for FakeCommands {
        fn install(&mut self,_:&Path,root:&Path)->Result<(),String>{assert_eq!(root,self.root);for name in FILES{fs::write(root.join(name),format!("new-{name}")).unwrap();}if self.install_failure{Err("mock installer failure".into())}else{Ok(())}}
        fn start(&mut self,exe:&Path,id:Option<&str>)->Result<Box<dyn Running>,String>{assert_eq!(exe,self.root.join("unreal-manager.exe"));self.starts+=1;if let Some(id)=id{if self.healthy{json(&self.job.join("healthy.json"),&Healthy{id:id.into(),version:self.version.clone(),app_sha256:self.sha.clone()})?;}}Ok(Box::new(FakeRunning{stopped:self.stopped.clone()}))}
    }
    #[test]fn portable_commit_preserves_account_marker_and_unknown_files(){let mut f=Fixture::new("portable");let mut commands=FakeCommands::for_fixture(&f,true);execute(&mut f.journal,Path::new("unused"),&mut commands,Duration::from_millis(5)).unwrap();for name in FILES{assert_eq!(fs::read(f.root.join(name)).unwrap(),format!("new-{name}").as_bytes());}f.preserved();assert!(!f.root.join(JOURNAL).exists());assert_eq!(commands.starts,1);}
    #[test]fn health_timeout_stops_only_owned_child_rolls_back_and_restarts_old(){let mut f=Fixture::new("portable");let mut commands=FakeCommands::for_fixture(&f,false);assert!(execute(&mut f.journal,Path::new("unused"),&mut commands,Duration::from_millis(1)).is_err());f.old();assert!(commands.stopped.get());assert_eq!(commands.starts,2);}
    #[test]fn installed_failure_restores_same_path_files_without_registry_or_native_installer(){let mut f=Fixture::new("installed");let mut commands=FakeCommands::for_fixture(&f,false);commands.install_failure=true;assert!(execute(&mut f.journal,Path::new("mock-installer.exe"),&mut commands,Duration::from_millis(1)).is_err());f.old();assert_eq!(commands.starts,1);}
    #[test]fn installed_success_uses_same_path_and_compatible_health_payload(){let mut f=Fixture::new("installed");let mut commands=FakeCommands::for_fixture(&f,true);execute(&mut f.journal,Path::new("mock-installer.exe"),&mut commands,Duration::from_millis(1)).unwrap();f.preserved();assert_eq!(commands.starts,1);}
    fn interrupted(f:&mut Fixture){f.journal.state="applying".into();for op in &mut f.journal.operations{op.new_hash=None;}save(&f.journal).unwrap();}
    #[test]fn explicit_installed_repair_archives_unknown_outputs_before_restoring_verified_old(){
        let mut f=Fixture::new("installed");interrupted(&mut f);
        fs::write(f.root.join("unreal-manager.exe"),b"partial unknown executable").unwrap();fs::write(f.root.join("README.txt"),b"external edited readme").unwrap();fs::remove_file(f.root.join("LICENSE")).unwrap();
        assert!(crash_recovery_allowed(&f.journal).is_err());let preview=repair_preview(&f.journal).unwrap();let archive=restore_verified_old(&f.journal,&preview).unwrap();
        f.old();assert!(!f.root.join(JOURNAL).exists());assert!(archive.join("original-journal.json").is_file());assert!(archive.join("archive-complete").is_file());
        assert_eq!(fs::read(archive.join("current/unreal-manager.exe")).unwrap(),b"partial unknown executable");assert_eq!(fs::read(archive.join("current/README.txt")).unwrap(),b"external edited readme");assert!(!archive.join("current/LICENSE").exists());
        for name in FILES{assert_eq!(fs::read(stage(&f.root,&f.journal.id).join("backup").join(name)).unwrap(),format!("old-{name}").as_bytes());}
    }
    #[test]fn explicit_repair_can_restore_a_missing_executable_and_preserves_user_data(){let mut f=Fixture::new("installed");interrupted(&mut f);fs::remove_file(&f.journal.original_exe).unwrap();let preview=repair_preview(&f.journal).unwrap();restore_verified_old(&f.journal,&preview).unwrap();f.old();}
    #[test]fn repair_refuses_corrupt_backup_and_postapproval_external_edit_without_replacing_files(){
        let mut f=Fixture::new("installed");interrupted(&mut f);let preview=repair_preview(&f.journal).unwrap();fs::write(f.root.join("README.txt"),b"new external edit").unwrap();
        assert!(restore_verified_old(&f.journal,&preview).is_err());assert!(!preview.archive.exists());assert_eq!(fs::read(f.root.join("README.txt")).unwrap(),b"new external edit");assert!(f.root.join(JOURNAL).exists());
        fs::write(stage(&f.root,&f.journal.id).join("backup/LICENSE"),b"corrupt backup").unwrap();assert!(repair_preview(&f.journal).is_err());assert_eq!(fs::read(f.root.join("unreal-manager.exe")).unwrap(),b"old-unreal-manager.exe");f.preserved();
    }
    #[test]fn repair_rejects_changed_journal_or_archive_outside_owned_stage(){let mut f=Fixture::new("installed");interrupted(&mut f);let mut preview=repair_preview(&f.journal).unwrap();preview.archive=f.root.join("Data/repair");assert!(restore_verified_old(&f.journal,&preview).is_err());let preview=repair_preview(&f.journal).unwrap();f.journal.state="needsRecovery".into();save(&f.journal).unwrap();assert!(restore_verified_old(&f.journal,&preview).is_err());f.old();}
    #[cfg(windows)]#[test]fn interrupted_repair_keeps_archive_and_backups_then_accepts_a_new_explicit_approval(){
        let mut f=Fixture::new("installed");interrupted(&mut f);fs::write(f.root.join("unreal-manager.exe"),b"partial exe").unwrap();fs::write(f.root.join("README.txt"),b"partial readme").unwrap();let preview=repair_preview(&f.journal).unwrap();
        let guard=lock_input(&f.root.join("README.txt")).unwrap();assert!(restore_verified_old(&f.journal,&preview).is_err());drop(guard);
        assert_eq!(fs::read(f.root.join("unreal-manager.exe")).unwrap(),b"old-unreal-manager.exe");assert_eq!(fs::read(f.root.join("README.txt")).unwrap(),b"partial readme");assert_eq!(fs::read(preview.archive.join("current/unreal-manager.exe")).unwrap(),b"partial exe");assert!(f.root.join(JOURNAL).exists());f.preserved();
        let retry=repair_preview(&f.journal).unwrap();assert_ne!(retry.archive,preview.archive);restore_verified_old(&f.journal,&retry).unwrap();f.old();assert!(preview.archive.join("archive-complete").is_file());
    }
    #[cfg(windows)]#[test]fn approved_current_archive_cannot_be_changed_while_repair_replaces_targets(){
        let mut f=Fixture::new("installed");interrupted(&mut f);
        fs::write(f.root.join("README.txt"),b"retained external edit").unwrap();
        fs::write(f.root.join("LICENSE"),b"partial installer output").unwrap();
        let preview=repair_preview(&f.journal).unwrap();
        // Block a later target replacement to observe the completed archive
        // during restoration rather than after all read guards have dropped.
        let blocked=lock_input(&f.root.join("LICENSE")).unwrap();let archive=preview.archive.clone();
        let observer=std::thread::spawn(move||{
            let start=Instant::now();while !archive.join("archive-complete").exists(){assert!(start.elapsed()<Duration::from_secs(2),"archive was not completed");std::thread::sleep(Duration::from_millis(5));}
            let retained=archive.join("current/README.txt");
            assert!(OpenOptions::new().write(true).open(&retained).is_err());
            assert!(fs::remove_file(&retained).is_err());drop(blocked);
        });
        let restored=restore_verified_old(&f.journal,&preview);observer.join().unwrap();restored.unwrap();f.old();
        assert_eq!(fs::read(preview.archive.join("current/README.txt")).unwrap(),b"retained external edit");
    }
    #[test]fn live_signed_installer_bypass_requires_exact_payload_and_owned_live_ancestry(){let expected=Path::new("X:/Fixture/Setup.exe");assert!(installer_claim_matches(expected,expected,"signed","signed",true,true));assert!(!installer_claim_matches(Path::new("X:/Other/Setup.exe"),expected,"signed","signed",true,true));assert!(!installer_claim_matches(expected,expected,"edited","signed",true,true));assert!(!installer_claim_matches(expected,expected,"signed","signed",false,true));assert!(!installer_claim_matches(expected,expected,"signed","signed",true,false));}
    #[test]fn repair_arguments_require_explicit_installer_no_restart_and_bounded_parent_identity(){let args=|values:&[&str]|values.iter().map(std::ffi::OsString::from).collect();assert!(repair_arguments(args(&[JOURNAL,"--installer","Setup.exe"])).is_err());assert!(repair_arguments(args(&[JOURNAL,"--wait-parent","0","1"])).is_err());assert!(repair_arguments(args(&[JOURNAL,"--wait-parent","10","0"])).is_err());assert!(repair_arguments(args(&[JOURNAL,"--no-restart","--no-restart"])).is_err());let parsed=repair_arguments(args(&[JOURNAL,"--no-restart","--installer","Setup.exe"])).unwrap();assert!(parsed.no_restart);}
    #[test]fn ui_health_ack_is_once_after_success_and_failure_does_not_claim_ready(){let flag=AtomicBool::new(false);assert!(acknowledge_ui_once(&flag,||Err("startup not ready".into())).is_err());assert!(!flag.load(Ordering::SeqCst));assert_eq!(acknowledge_ui_once(&flag,||Ok(())).unwrap(),true);assert_eq!(acknowledge_ui_once(&flag,||panic!("duplicate acknowledgement")).unwrap(),false);let payload=serde_json::to_value(Healthy{id:"fixture".into(),version:"1.0.0".into(),app_sha256:"hash".into()}).unwrap();assert_eq!(payload,serde_json::json!({"id":"fixture","version":"1.0.0","appSha256":"hash"}));}
    #[test]fn rollback_preserves_external_edit_and_retains_backup(){let f=Fixture::new("portable");portable_apply(&f.journal).unwrap();fs::write(f.root.join("README.txt"),b"external-edit").unwrap();assert!(rollback(&f.journal).is_err());assert_eq!(fs::read(f.root.join("README.txt")).unwrap(),b"external-edit");assert!(stage(&f.root,&f.journal.id).join("backup/unreal-manager.exe").is_file());f.preserved();}
    #[test]fn partial_commit_rolls_back_after_process_crash(){let f=Fixture::new("portable");fs::rename(stage(&f.root,&f.journal.id).join("new/unreal-manager.exe"),f.root.join("unreal-manager.exe")).unwrap();rollback(&f.journal).unwrap();f.old();}
    #[test]fn changed_portable_marker_blocks_recovery(){let f=Fixture::new("portable");fs::write(f.root.join("portable.marker"),b"changed-marker").unwrap();assert!(rollback(&f.journal).is_err());assert!(f.root.join(JOURNAL).exists());}
    #[test]fn cleanup_preflight_preserves_every_backup_when_unknown_file_exists(){let f=Fixture::new("portable");fs::write(stage(&f.root,&f.journal.id).join("new/unexpected"),b"external").unwrap();assert!(clean(&f.journal).is_err());for name in FILES{assert!(stage(&f.root,&f.journal.id).join("backup").join(name).is_file());}f.old();}
    #[test]fn cleanup_preserves_changed_backup(){let f=Fixture::new("portable");let path=stage(&f.root,&f.journal.id).join("backup/LICENSE");fs::write(&path,b"external edit").unwrap();assert!(clean(&f.journal).is_err());assert_eq!(fs::read(path).unwrap(),b"external edit");assert!(stage(&f.root,&f.journal.id).join("backup/unreal-manager.exe").is_file());}
    #[test]fn interrupted_installer_with_unobserved_file_keeps_backup(){let mut f=Fixture::new("installed");for op in &mut f.journal.operations{op.new_hash=None;}f.journal.state="applying".into();fs::write(f.root.join("README.txt"),b"unknown interrupted Inno output").unwrap();assert!(rollback(&f.journal).is_err());assert!(stage(&f.root,&f.journal.id).join("backup/unreal-manager.exe").is_file());f.preserved();}
    #[test]fn preparing_with_partial_backups_can_clean_without_changing_targets(){let f=Fixture::new("portable");fs::remove_file(stage(&f.root,&f.journal.id).join("backup/LICENSE")).unwrap();rollback(&f.journal).unwrap();clean(&f.journal).unwrap();f.old();}
    #[test]fn truncated_backup_retains_evidence_and_restarts_verified_old_files(){let mut f=Fixture::new("portable");fs::write(stage(&f.root,&f.journal.id).join("backup/unreal-manager.exe"),b"truncated").unwrap();let mut commands=FakeCommands::for_fixture(&f,false);assert!(restore_after_failure(&mut f.journal,&mut commands,"preparation interrupted").is_err());f.old();assert_eq!(commands.starts,1);assert!(!f.root.join(JOURNAL).exists());assert_eq!(fs::read(stage(&f.root,&f.journal.id).join("backup/unreal-manager.exe")).unwrap(),b"truncated");assert!(fs::read_dir(stage(&f.root,&f.journal.id)).unwrap().any(|entry|entry.unwrap().file_name().to_string_lossy().starts_with("retained-recovery-")));}
    #[test]fn unobserved_interrupted_installer_never_auto_recovers_even_pristine_targets(){let mut f=Fixture::new("installed");for op in &mut f.journal.operations{op.new_hash=None;}f.journal.state="applying".into();assert!(crash_recovery_allowed(&f.journal).is_err());f.old();assert!(f.root.join(JOURNAL).is_file());f.journal.state="preparing".into();assert!(crash_recovery_allowed(&f.journal).is_ok());}
    #[cfg(windows)]#[test]fn sharing_lock_is_retried_without_overwriting_external_changes(){use std::os::windows::fs::OpenOptionsExt;let f=Fixture::new("portable");let target=f.root.join("README.txt");let source=stage(&f.root,&f.journal.id).join("new/README.txt");let operation=f.journal.operations.iter().find(|op|op.name=="README.txt").unwrap();let guard=OpenOptions::new().read(true).share_mode(1).open(&target).unwrap();let thread=std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(150));drop(guard);});replace_file(&source,&target,&operation.new_hash,&operation.old_hash).unwrap();thread.join().unwrap();assert_eq!(fs::read(&target).unwrap(),b"new-README.txt");f.preserved();}
    #[test]fn installer_dir_accepts_canonical_disk_and_unc_paths(){assert_eq!(installer_directory(Path::new(r"\\?\X:\Fixture\App")),r"X:\Fixture\App");assert_eq!(installer_directory(Path::new(r"\\?\UNC\server\share\App")),r"\\server\share\App");}
    #[test]fn bundle_rejects_data_marker_traversal_duplicates_and_wrong_version(){
        let make=||Bundle{schema:1,version:"0.2.0".into(),files:FILES.iter().map(|name|BundleFile{name:(*name).into(),data:base64::engine::general_purpose::STANDARD.encode(b"fixture")}).collect()};
        assert!(decode_bundle(make(),"0.2.0").is_ok());assert!(decode_bundle(make(),"0.3.0").is_err());
        for bad in ["Data/session","portable.marker","../unreal-manager.exe","LICENSE:secret","readme.txt"]{let mut b=make();b.files[0].name=bad.into();assert!(decode_bundle(b,"0.2.0").is_err());}
        let mut duplicate=make();duplicate.files[1].name="unreal-manager.exe".into();assert!(decode_bundle(duplicate,"0.2.0").is_err());
    }
    #[cfg(windows)]#[test]fn process_identity_uses_exact_creation_and_image_and_missing_pid_is_not_reused(){let (created,image)=process_identity(std::process::id()).unwrap();assert!(created>0);assert!(same(&image,&canonical(&std::env::current_exe().unwrap()).unwrap()));assert!(windows::Process::open(u32::MAX).unwrap().is_none());}
    #[cfg(windows)]#[test]fn authenticated_input_handle_denies_write_and_delete(){let f=Fixture::new("portable");let path=f.root.join("README.txt");let _guard=lock_input(&path).unwrap();assert!(OpenOptions::new().write(true).open(&path).is_err());assert!(fs::remove_file(&path).is_err());assert_eq!(fs::read(&path).unwrap(),b"old-README.txt");}
    /// Launched only by the opt-in verification fixture. This entry is absent
    /// from production EXEs; it invokes the same signed native helper logic.
    #[cfg(windows)]#[test]#[ignore]
    fn native_subprocess_entry(){
        let mode=std::env::var("UM_UPDATE_FIXTURE_MODE").expect("explicit native fixture mode");
        let directory=PathBuf::from(std::env::var_os("UM_UPDATE_FIXTURE_DIRECTORY").expect("fixture directory"));
        match mode.as_str(){
            "parent"=>{write_new(&directory.join("parent-ready"),b"ready").unwrap();let start=Instant::now();while !directory.join("parent-exit").exists(){assert!(start.elapsed()<Duration::from_secs(90),"fixture parent deadline");std::thread::sleep(Duration::from_millis(20));}},
            "helper"=>{run(&directory.join("job.json")).unwrap();},
            "old"=>{assert!(!recover_or_busy().unwrap(),"restored startup must not see a busy helper");write_new(&directory.join("old-restarted"),b"restarted").unwrap();},
            "expect-interrupted-installer"=>{let error=recover_or_busy().unwrap_err();assert!(error.contains("Interrupted installer"),"{error}");write_new(&directory.join("interrupted-installer-blocked"),b"blocked").unwrap();},
            _=>panic!("unknown native fixture mode"),
        }
    }
}

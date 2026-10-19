#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args_os().skip(1);
    let mode=args.next();
    if mode.as_deref()==Some(std::ffi::OsStr::new("--apply-app-update")) {
        let result=args.next().ok_or_else(||"Missing update job".to_string()).and_then(|p|unreal_manager_lib::run_app_update_helper(p.into()));
        if let Err(error)=result { eprintln!("{error}"); std::process::exit(1); }
        return;
    }
    #[cfg(any(debug_assertions, feature = "engine-test-cli"))]
    if mode.as_deref()==Some(std::ffi::OsStr::new("--engine-saved-task-inspect")) { unreal_manager_lib::engine_saved_task_inspect(args.next().expect("tasks folder").into(),args.next().expect("task id").to_string_lossy().into_owned(),args.next().expect("report path").into()); return; }
    #[cfg(debug_assertions)]
    if mode.as_deref()==Some(std::ffi::OsStr::new("--fab-info-report")) { unreal_manager_lib::fab_info_report(args.next().expect("report path").into()); return; }
    #[cfg(any(debug_assertions, feature = "engine-test-cli"))]
    if mode.as_deref()==Some(std::ffi::OsStr::new("--engine-resume-inspect")) { unreal_manager_lib::engine_resume_inspect(args.next().expect("engine path").into(),args.next().expect("report path").into()); return; }
    #[cfg(any(debug_assertions, feature = "engine-test-cli"))]
    if mode.as_deref()==Some(std::ffi::OsStr::new("--engine-install-local")) { unreal_manager_lib::engine_test_install(args.next().expect("manifest path").into(),args.next().expect("destination folder").into(),args.next().expect("report path").into()); return; }
    if mode.as_deref()==Some(std::ffi::OsStr::new("--engine-plan-report")) { unreal_manager_lib::engine_plan_report(args.next().expect("manifest path").into(),args.next().expect("destination folder").into(),args.next().expect("report path").into()); return; }
    if mode.as_deref()==Some(std::ffi::OsStr::new("--engine-report")) { unreal_manager_lib::engine_report(args.next().expect("output path").into()); return; }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--scan-report")) {
        let path = args.next().expect("--scan-report requires an output path");
        unreal_manager_lib::write_scan_report(std::path::Path::new(&path))
            .expect("failed to write scan report");
        return;
    }
    if mode.as_deref()==Some(std::ffi::OsStr::new("--repair-engine-registration")) {
        let result=unreal_manager_lib::repair_engine_registrations();
        let report=args.next().expect("report path");
        std::fs::write(report,serde_json::to_vec_pretty(&result).unwrap()).expect("report write");return;
    }
    match unreal_manager_lib::app_update_startup() {
        Ok(true)=>return,
        Ok(false)=>{},
        Err(error)=>{unreal_manager_lib::report_app_update_error(&error);return;}
    }
    // WebView2 normally uses Chromium GPU compositing. For this launcher the
    // default requests software rendering. Validate GPU activity on each
    // target system; Chromium flags alone do not prove a hardware power state.
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-gpu --disable-gpu-compositing"
    );

    unreal_manager_lib::run();
}





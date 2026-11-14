//! ADR 0089: a double-click on `kim.exe` hands over to the Kim window; nothing else does.
//!
//! A double-click is simulated without the desktop: `kim.exe` is started alone on a new console
//! whose window is hidden (`CREATE_NEW_CONSOLE` + `SW_HIDE`), with no arguments. Next to this
//! copy of `kim.exe` sits a stand-in `kim-manager.exe` (another copy of `kim.exe`: it gets the
//! window's `--catalog` argument, refuses it as a CLI usage error and ends) and a local mirror.
//! Every start runs inside a job object, created suspended, so the job's accounting counts
//! what `kim` starts. The same `kim.exe` with an argument (`--version`) is the baseline: it
//! starts nothing. Nothing becomes visible: the console window is hidden and the stand-in has
//! no window.
#![cfg(windows)]
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JobObjectBasicAccountingInformation, QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_CONSOLE, CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess, INFINITE,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESHOWWINDOW, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};

const SW_HIDE: u16 = 0;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Runs `exe args` alone on a new, hidden console inside a job; returns (exit code, processes
/// the job ever held). A run that does not end in 10 s is ended and reported as `None`.
fn run_hidden(exe: &Path, args: &str, env: &[(&str, &str)]) -> (Option<u32>, u32) {
    let mut cmdline = wide(&format!("\"{}\"{args}", exe.display()));
    // The environment block: this process's, with `env` added (UTF-16, double-NUL ended).
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.retain(|(k, _)| !k.eq_ignore_ascii_case("KIM_NO_INPUT"));
    for (k, v) in env {
        vars.push(((*k).into(), (*v).into()));
    }
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in vars {
        block.extend(format!("{k}={v}").encode_utf16());
        block.push(0);
    }
    block.push(0);
    let si = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap(),
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: anonymous job with default security; checked for null; closed below.
    let job: HANDLE = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!job.is_null());
    let dir = wide(&exe.parent().unwrap().display().to_string());
    // SAFETY: every pointer is valid for the call: a writable command line, a double-NUL ended
    // Unicode environment block, a NUL-ended directory, and initialized STARTUPINFOW and
    // PROCESS_INFORMATION structures.
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_SUSPENDED | 0x0000_0400, // CREATE_UNICODE_ENVIRONMENT
            block.as_ptr().cast(),
            dir.as_ptr(),
            &raw const si,
            &raw mut pi,
        )
    };
    assert_ne!(ok, 0, "CreateProcessW: {}", std::io::Error::last_os_error());
    // SAFETY: valid handles from CreateProcessW; the process has not run yet.
    unsafe {
        assert_ne!(AssignProcessToJobObject(job, pi.hProcess), 0, "job");
        ResumeThread(pi.hThread);
    }
    // SAFETY: a valid process handle.
    let waited = unsafe { WaitForSingleObject(pi.hProcess, 10_000) };
    let code = if waited == WAIT_OBJECT_0 {
        let mut c = 0u32;
        // SAFETY: a valid process handle and a writable u32.
        unsafe { GetExitCodeProcess(pi.hProcess, &raw mut c) };
        Some(c)
    } else {
        // SAFETY: a valid process handle; this run is ours.
        unsafe {
            TerminateProcess(pi.hProcess, 1);
            WaitForSingleObject(pi.hProcess, INFINITE);
        }
        None
    };
    // Give a started stand-in time to end (it fails at once on its arguments).
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: `info` is a writable struct of exactly the size passed; handles are closed once.
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).unwrap(),
            std::ptr::null_mut(),
        );
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        CloseHandle(job);
    }
    (code, info.TotalProcesses)
}

#[test]
fn a_double_click_opens_the_window_and_nothing_else_does() {
    let d = Dir(std::env::temp_dir().join(format!("kim-dblclick-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&d.0);
    std::fs::create_dir_all(d.0.join("kim-local")).unwrap();
    let kim = d.0.join("kim.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_kim"), &kim).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_kim"), d.0.join("kim-manager.exe")).unwrap();
    std::fs::write(d.0.join("kim-local").join("catalog.json"), "{}").unwrap();
    std::fs::write(d.0.join("kim-local").join("catalog.json.sig"), "{}").unwrap();
    // Kim's own data in the sandbox (no installed Kim is looked at).
    let local = d.0.join("local");
    let roaming = d.0.join("roaming");
    let env = [
        ("LOCALAPPDATA", local.to_str().unwrap()),
        ("APPDATA", roaming.to_str().unwrap()),
    ];

    let (code, baseline) = run_hidden(&kim, " --version", &env);
    assert_eq!(code, Some(0), "kim --version");
    let (code, clicked) = run_hidden(&kim, "", &env);
    assert_eq!(code, Some(0), "the double-click start ends at once");
    assert_eq!(
        clicked,
        baseline + 1,
        "a double-click starts exactly one program: the window next to kim.exe"
    );
    let (_, off) = run_hidden(&kim, "", &[env[0], env[1], ("KIM_NO_INPUT", "1")]);
    assert_eq!(off, baseline, "KIM_NO_INPUT=1 turns the hand-over off");

    // Without a window next to it (and none installed), the console stays open with help and
    // a note until Enter: still running after 10 s, and it started nothing.
    std::fs::remove_file(d.0.join("kim-manager.exe")).unwrap();
    let (code, alone) = run_hidden(&kim, "", &env);
    assert_eq!(code, None, "waits for Enter");
    assert_eq!(alone, baseline, "nothing started");
    println!(
        "EVIDENCE double-click: baseline {baseline} process(es), double-click {clicked} (+ the window), KIM_NO_INPUT {off}, no window {alone}"
    );
}

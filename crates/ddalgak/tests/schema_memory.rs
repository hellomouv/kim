//! #87: validating a rules document must not keep copies of the schema resident. Every
//! sub-schema validator carried its own copy of `$defs` (about 25 MB in the engine host, found
//! by the real-desktop memory check); they now share one. A test binary of its own, so the
//! process's private bytes measure this alone.
#![cfg(windows)]
#![allow(unsafe_code)]

#[repr(C)]
#[derive(Default)]
struct Counters {
    cb: u32,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_non_paged: usize,
    quota_non_paged: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    fn GetCurrentProcess() -> isize;
}

fn private_mib() -> f64 {
    let mut c = Counters {
        cb: u32::try_from(size_of::<Counters>()).unwrap(),
        ..Default::default()
    };
    // SAFETY: the current process's pseudo handle and a sized out-structure.
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut c, c.cb) };
    c.private as f64 / 1_048_576.0
}

#[test]
fn validating_a_document_keeps_no_schema_copies_resident() {
    let text = std::fs::read_to_string(
        "../../docs/contracts/examples/ddalgak-rules-v1/04-open-website.json",
    )
    .unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let before = private_mib();
    let errors = ddalgak::schema::document_errors(&doc);
    assert!(errors.is_empty(), "{errors:?}");
    // Validating once more reuses everything.
    let _ = ddalgak::schema::document_errors(&doc);
    let grown = private_mib() - before;
    assert!(
        grown < 8.0,
        "validation kept {grown:.1} MiB resident (it was about 25 MiB with a $defs copy per sub-schema)"
    );
}

//! #98 T3 (`CORE-78`): the shown Terminal's progress (OSC 9;4) on the window's taskbar button,
//! through `ITaskbarList3` (called through its vtable: `windows-sys` has no COM wrappers).
//! The off-screen test window never touches the taskbar; it reports the state instead.

use std::ffi::c_void;
use std::ptr::null_mut;
use subak_term::screen::{Progress, ProgressState};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_sys::core::{GUID, HRESULT};

static CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
static IID_ITASKBAR_LIST3: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);

/// `TBPFLAG` values.
const TBPF_NOPROGRESS: u32 = 0;
const TBPF_INDETERMINATE: u32 = 1;
const TBPF_NORMAL: u32 = 2;
const TBPF_ERROR: u32 = 4;
const TBPF_PAUSED: u32 = 8;

/// The taskbar flag and the value (of 100) for a progress.
pub fn flags(p: Option<Progress>) -> (u32, Option<u64>) {
    match p {
        None => (TBPF_NOPROGRESS, None),
        Some(p) => match p.state {
            ProgressState::Normal => (TBPF_NORMAL, Some(u64::from(p.value))),
            ProgressState::Error => (TBPF_ERROR, Some(u64::from(p.value))),
            ProgressState::Paused => (TBPF_PAUSED, Some(u64::from(p.value))),
            ProgressState::Indeterminate => (TBPF_INDETERMINATE, None),
        },
    }
}

/// An `ITaskbarList3` of this thread (the window's UI thread).
pub struct Taskbar {
    this: *mut c_void,
}

impl std::fmt::Debug for Taskbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Taskbar")
    }
}

type HrInit = unsafe extern "system" fn(*mut c_void) -> HRESULT;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;
type SetValue = unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> HRESULT;
type SetState = unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT;

impl Taskbar {
    /// `None` when the shell has no taskbar list (no Explorer, a server core).
    pub fn new() -> Option<Self> {
        let mut this: *mut c_void = null_mut();
        // SAFETY: COM on this thread (S_FALSE / RPC_E_CHANGED_MODE leave it usable), then an
        // in-process object whose pointer we own and release in `drop`.
        unsafe {
            CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32);
            let hr = CoCreateInstance(
                &raw const CLSID_TASKBAR_LIST,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &raw const IID_ITASKBAR_LIST3,
                &raw mut this,
            );
            if hr < 0 || this.is_null() {
                return None;
            }
            let t = Self { this };
            if (t.slot::<HrInit>(3))(this) < 0 {
                return None;
            }
            Some(t)
        }
    }

    /// The function in vtable slot `i`.
    unsafe fn slot<F: Copy>(&self, i: usize) -> F {
        // SAFETY: `this` is a live ITaskbarList3; its vtable has at least 11 slots
        // (IUnknown 0–2, ITaskbarList 3–7, ITaskbarList2 8, ITaskbarList3 9–).
        unsafe {
            let vtbl = *(self.this as *const *const usize);
            std::mem::transmute_copy(&*vtbl.add(i))
        }
    }

    /// Shows `p` on `hwnd`'s taskbar button (nothing: no progress).
    pub fn set(&self, hwnd: HWND, p: Option<Progress>) {
        let (flag, value) = flags(p);
        // SAFETY: ITaskbarList3::SetProgressState (10) and SetProgressValue (9) on our window.
        unsafe {
            (self.slot::<SetState>(10))(self.this, hwnd, flag);
            if let Some(v) = value {
                (self.slot::<SetValue>(9))(self.this, hwnd, v, 100);
            }
        }
    }
}

impl Drop for Taskbar {
    fn drop(&mut self) {
        // SAFETY: releases the reference `new` got.
        unsafe { (self.slot::<Release>(2))(self.this) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_states_map_to_taskbar_flags() {
        let p = |state, value| Some(Progress { state, value });
        assert_eq!(flags(None), (TBPF_NOPROGRESS, None));
        assert_eq!(flags(p(ProgressState::Normal, 42)), (TBPF_NORMAL, Some(42)));
        assert_eq!(flags(p(ProgressState::Error, 7)), (TBPF_ERROR, Some(7)));
        assert_eq!(flags(p(ProgressState::Paused, 9)), (TBPF_PAUSED, Some(9)));
        assert_eq!(
            flags(p(ProgressState::Indeterminate, 0)),
            (TBPF_INDETERMINATE, None)
        );
    }
}

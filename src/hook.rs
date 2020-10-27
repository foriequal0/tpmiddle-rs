use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use anyhow::*;
use winapi::_core::marker::PhantomData;
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{DWORD, HMODULE, LPARAM, LRESULT, WPARAM};
use winapi::shared::ntdef::{LPCWSTR, NULL};
use winapi::shared::windef::HHOOK;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HOOKPROC, WH_MOUSE_LL,
};

pub enum HookProcError {
    UnhandledMessage,
}

pub type HookProcResult = Result<LRESULT, HookProcError>;

pub trait HookProc {
    fn proc(&mut self, n_code: c_int, w_param: WPARAM, l_param: LPARAM) -> HookProcResult;
}

pub struct LowLevelMouseHook<H> {
    hook_handle: Option<HookHandle>,
    hook_proc_handle: Option<HookProcHandle>,
    _phantom: PhantomData<H>,
}

impl<H: HookProc + 'static> LowLevelMouseHook<H> {
    pub fn new(hook_proc: H) -> Result<Self> {
        extern "system" fn proc(n_code: c_int, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
            let mut hooks = HOOKS.lock().unwrap();
            let entry = hooks.get_mut(&WH_MOUSE_LL).unwrap();
            if n_code < 0 {
                unsafe { CallNextHookEx(entry.hhook, n_code, w_param, l_param) }
            } else {
                match entry.proc.proc(n_code, w_param, l_param) {
                    Ok(value) => value,
                    Err(HookProcError::UnhandledMessage) => unsafe {
                        CallNextHookEx(entry.hhook, n_code, w_param, l_param)
                    },
                }
            }
        }

        let register = register_hook_proc_context(WH_MOUSE_LL, hook_proc)?;
        let hmod = c_try_nonnull!(GetModuleHandleW(NULL as LPCWSTR))?;
        let hook_handle = HookHandle::new(WH_MOUSE_LL, Some(proc), hmod, 0)?;
        let hook_proc_handle = register.get_handle(hook_handle.hhook);

        Ok(Self {
            hook_handle: Some(hook_handle),
            hook_proc_handle: Some(hook_proc_handle),
            _phantom: Default::default(),
        })
    }
}

impl<H> Drop for LowLevelMouseHook<H> {
    fn drop(&mut self) {
        self.hook_handle.take();
        self.hook_proc_handle.take();
    }
}

struct HookHandle {
    hhook: HHOOK,
}

impl HookHandle {
    fn new(id_hook: c_int, lpfn: HOOKPROC, hmod: HMODULE, dw_thread_id: DWORD) -> Result<Self> {
        let hhook = c_try_nonnull!(SetWindowsHookExW(id_hook, lpfn, hmod, dw_thread_id))?;
        Ok(Self { hhook })
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        c_try!(UnhookWindowsHookEx(self.hhook)).unwrap();
    }
}

struct RegisteredHookProc {
    proc: Box<dyn HookProc>,
    hhook: HHOOK,
}

unsafe impl Send for RegisteredHookProc {}

lazy_static! {
    static ref HOOKS: Mutex<HashMap<c_int, RegisteredHookProc>> = Mutex::new(HashMap::new());
}

#[must_use]
struct HookProcRegisterGuard {
    hooks_guard: MutexGuard<'static, HashMap<c_int, RegisteredHookProc>>,
    id_hook: c_int,
}

fn register_hook_proc_context<H: HookProc + 'static>(
    id_hook: c_int,
    hook_proc: H,
) -> Result<HookProcRegisterGuard> {
    let mut hooks = HOOKS.lock().unwrap();
    if hooks.contains_key(&id_hook) {
        bail!("Hook is already registered for {:X}", id_hook);
    }
    hooks.insert(
        id_hook,
        RegisteredHookProc {
            proc: Box::new(hook_proc),
            hhook: std::ptr::null_mut(),
        },
    );
    Ok(HookProcRegisterGuard {
        hooks_guard: hooks,
        id_hook,
    })
}

impl HookProcRegisterGuard {
    fn get_handle(mut self, hhook: HHOOK) -> HookProcHandle {
        self.hooks_guard.get_mut(&self.id_hook).unwrap().hhook = hhook;
        HookProcHandle {
            id_hook: self.id_hook,
        }
    }
}

struct HookProcHandle {
    id_hook: c_int,
}

impl Drop for HookProcHandle {
    fn drop(&mut self) {
        let mut hooks = HOOKS.lock().unwrap();
        assert!(hooks.contains_key(&self.id_hook));
        hooks.remove(&self.id_hook);
    }
}

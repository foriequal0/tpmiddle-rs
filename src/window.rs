use std::collections::HashSet;
use std::ffi::OsStr;
use std::iter::{once, Iterator};
use std::os::windows::ffi::OsStrExt;

use anyhow::*;
use log::*;
use thiserror::*;
use winapi::_core::marker::PhantomData;
use winapi::ctypes::wchar_t;
use winapi::shared::basetsd::LONG_PTR;
use winapi::shared::minwindef::{
    FALSE, HINSTANCE, INT, LPARAM, LPVOID, LRESULT, TRUE, UINT, WPARAM,
};
use winapi::shared::ntdef::{LPCWSTR, LPWSTR, NULL};
use winapi::shared::windef::{HMENU, HWND};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wincon::GetConsoleWindow;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, IsWindowVisible,
    PostQuitMessage, RegisterClassExW, RegisterRawInputDevices, SetWindowLongPtrW, ShowWindow,
    UnregisterClassW, HWND_MESSAGE, RAWINPUTDEVICE, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDEV_REMOVE,
    SW_HIDE, WNDCLASSEXW,
};

use crate::hid::DEVICE_INFOS;

pub struct Window<T> {
    _class: WindowClass<T>,
    hwnd: HWND,
    _phantom: PhantomData<T>,
}

impl<T: WindowProc> Window<T> {
    pub fn new(proc: T) -> Result<Self> {
        let mut proc = Box::new(proc);
        let class = WindowClass::new()?;
        let hwnd = c_try_nonnull!(CreateWindowExW(
            0,
            class.atom,
            NULL as LPCWSTR,
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            NULL as HMENU,
            NULL as HINSTANCE,
            proc.as_mut() as *mut T as LPVOID,
        ))?;

        if let Err(err) = c_try_nonnull!(SetWindowLongPtrW(
            hwnd,
            0,
            proc.as_mut() as *mut T as LONG_PTR
        )) {
            c_try!(DestroyWindow(hwnd))?;
            return Err(err);
        }
        std::mem::forget(proc);

        unsafe {
            let console = GetConsoleWindow();
            if IsWindowVisible(console) == TRUE {
                ShowWindow(console, SW_HIDE);
            }
        }

        Ok(Window {
            _class: class,
            hwnd,
            _phantom: Default::default(),
        })
    }
}

impl<T> Drop for Window<T> {
    fn drop(&mut self) {
        unsafe {
            let ptr = window_proc_ptr_from_hwnd::<T>(self.hwnd);
            std::mem::drop(Box::from_raw(ptr));
            assert_ne!(0, DestroyWindow(self.hwnd));
        }
    }
}

#[derive(Error, Debug)]
pub enum WindowProcError {
    #[error("Unhandled message")]
    UnhandledMessage,

    #[error(transparent)]
    OtherError(#[from] anyhow::Error),
}

pub type WindowProcResult = Result<LRESULT, WindowProcError>;

pub trait WindowProc {
    fn proc(
        &mut self,
        hwnd: HWND,
        u_msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> WindowProcResult;
}

fn window_proc_ptr_from_hwnd<T>(hwnd: HWND) -> *mut T {
    unsafe {
        let long_ptr = GetWindowLongPtrW(hwnd, 0);
        std::mem::transmute::<LONG_PTR, *mut T>(long_ptr)
    }
}

extern "system" fn window_proc<T: WindowProc>(
    hwnd: HWND,
    u_msg: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let this = {
        let from_hwnd = window_proc_ptr_from_hwnd::<T>(hwnd);
        if from_hwnd.is_null() {
            l_param as *mut T
        } else {
            from_hwnd
        }
    };
    assert!(!this.is_null());
    unsafe {
        match (*this).proc(hwnd, u_msg, w_param, l_param) {
            Ok(result) => result,
            Err(WindowProcError::UnhandledMessage) => DefWindowProcW(hwnd, u_msg, w_param, l_param),
            Err(err) => {
                error!("{:?}", err);
                PostQuitMessage(-1);
                0
            }
        }
    }
}

struct WindowClass<T> {
    class: WNDCLASSEXW,
    atom: LPCWSTR,
    _phantom: PhantomData<T>,
}

impl<T: WindowProc> WindowClass<T> {
    fn new() -> Result<Self> {
        let name: Box<[wchar_t]> = OsStr::new("MessageWindowClass")
            .encode_wide()
            .chain(once(0))
            .collect();

        let mut class: WNDCLASSEXW = Default::default();
        class.cbSize = std::mem::size_of_val(&class) as UINT;
        class.cbWndExtra = std::mem::size_of::<*const T>() as INT;
        class.lpfnWndProc = Some(window_proc::<T>);
        class.hInstance = c_try_nonnull!(GetModuleHandleW(NULL as LPWSTR))?;
        class.lpszClassName = name.as_ptr();

        let atom = c_try_nonnull!(RegisterClassExW(&class))?;

        Ok(Self {
            class,
            atom: atom as LPCWSTR,
            _phantom: Default::default(),
        })
    }
}

impl<T> Drop for WindowClass<T> {
    fn drop(&mut self) {
        unsafe {
            assert_ne!(0, UnregisterClassW(self.atom, self.class.hInstance));
        }
    }
}

pub struct Devices {
    devices: Box<[RAWINPUTDEVICE]>,
}

impl Devices {
    pub fn new<T>(window: &Window<T>) -> Result<Self> {
        let mut usages = HashSet::new();
        for device_info in DEVICE_INFOS {
            usages.insert((device_info.usage_page, device_info.usage));
        }

        let mut devices = Vec::new();
        for (usage_page, usage) in usages {
            devices.push(RAWINPUTDEVICE {
                usUsagePage: usage_page,
                usUsage: usage,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: window.hwnd,
            });
        }

        c_try!(RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as UINT,
            std::mem::size_of::<RAWINPUTDEVICE>() as UINT
        ))?;

        Ok(Self {
            devices: devices.into(),
        })
    }
}

impl Drop for Devices {
    fn drop(&mut self) {
        for device in self.devices.iter_mut() {
            device.dwFlags = RIDEV_REMOVE;
            device.hwndTarget = NULL as HWND;
        }
        unsafe {
            assert_ne!(
                FALSE,
                RegisterRawInputDevices(
                    self.devices.as_ptr(),
                    self.devices.len() as UINT,
                    std::mem::size_of::<RAWINPUTDEVICE>() as UINT
                )
            );
        }
    }
}

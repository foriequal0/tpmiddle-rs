use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::iter::{once, Iterator};
use std::os::windows::ffi::OsStrExt;

use anyhow::*;
use log::*;
use thiserror::*;
use winapi::_core::marker::PhantomData;
use winapi::ctypes::wchar_t;
use winapi::shared::basetsd::LONG_PTR;
use winapi::shared::minwindef::{FALSE, LPARAM, LRESULT, TRUE, UINT, WPARAM};
use winapi::shared::ntdef::{LPCWSTR, NULL};
use winapi::shared::windef::HWND;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wincon::GetConsoleWindow;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, IsWindowVisible, PostQuitMessage, RegisterClassExW, RegisterRawInputDevices,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, UnregisterClassW, HWND_MESSAGE, MSG,
    RAWINPUTDEVICE, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDEV_REMOVE, SW_HIDE, WNDCLASSEXW,
};

use crate::hid::DeviceInfo;

pub struct Window<T> {
    _class: WindowClass<T>,
    pub hwnd: HWND,
    _phantom: PhantomData<T>,
}

impl<T: WindowProc> Window<T> {
    pub fn new(name: &str, proc: T) -> Result<Self> {
        let mut proc = Box::new(proc);
        let class = WindowClass::new(name)?;

        let name: Vec<wchar_t> = OsStr::new(name).encode_wide().chain(once(0)).collect();
        let hwnd = c_try_nonnull!(CreateWindowExW(
            0,
            class.atom,
            name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            NULL as _,
            NULL as _,
            proc.as_mut() as *mut T as _,
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

        Ok(Window {
            _class: class,
            hwnd,
            _phantom: Default::default(),
        })
    }

    pub fn run(self) -> Result<WPARAM> {
        let mut message: MSG = Default::default();
        loop {
            let status = c_try_ne!(-1, GetMessageW(&mut message, self.hwnd, 0, 0))?;
            if status == 0 {
                break Ok(message.wParam);
            }

            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
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
                eprintln!("Error: {:?}", err);
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
    fn new(name: &str) -> Result<Self> {
        let name: Vec<wchar_t> = OsStr::new(name).encode_wide().chain(once(0)).collect();

        let mut class: WNDCLASSEXW = Default::default();
        class.cbSize = std::mem::size_of_val(&class) as _;
        class.cbWndExtra = std::mem::size_of::<*const T>() as _;
        class.lpfnWndProc = Some(window_proc::<T>);
        class.hInstance = c_try_nonnull!(GetModuleHandleW(NULL as _))?;
        class.lpszClassName = name.as_ptr();

        let atom = c_try_nonnull!(RegisterClassExW(&class))?;

        Ok(Self {
            class,
            atom: atom as _,
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
    pub fn new<T>(
        window: &Window<T>,
        notify_devices: &[DeviceInfo],
        sink_devices: &[DeviceInfo],
    ) -> Result<Self> {
        let mut flags = HashMap::new();
        for device_info in notify_devices {
            flags.insert((device_info.usage_page, device_info.usage), RIDEV_DEVNOTIFY);
        }
        for device_info in sink_devices {
            match flags.entry((device_info.usage_page, device_info.usage)) {
                Entry::Occupied(mut entry) => *entry.get_mut() |= RIDEV_INPUTSINK,
                Entry::Vacant(entry) => {
                    entry.insert(RIDEV_INPUTSINK);
                }
            }
        }

        let mut devices = Vec::new();
        for ((usage_page, usage), flags) in flags {
            devices.push(RAWINPUTDEVICE {
                usUsagePage: usage_page,
                usUsage: usage,
                dwFlags: flags,
                hwndTarget: window.hwnd,
            });
        }

        c_try!(RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as _,
            std::mem::size_of::<RAWINPUTDEVICE>() as _
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
            device.hwndTarget = NULL as _;
        }
        unsafe {
            assert_ne!(
                FALSE,
                RegisterRawInputDevices(
                    self.devices.as_ptr(),
                    self.devices.len() as _,
                    std::mem::size_of::<RAWINPUTDEVICE>() as _
                )
            );
        }
    }
}

pub fn hide_console() {
    unsafe {
        let console = GetConsoleWindow();
        if IsWindowVisible(console) == TRUE {
            ShowWindow(console, SW_HIDE);
        }
    }
}

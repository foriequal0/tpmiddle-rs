use std::ffi::OsStr;
use std::iter::{once, Iterator};
use std::os::windows::ffi::OsStrExt;

use anyhow::*;
use winapi::_core::marker::PhantomData;
use winapi::ctypes::wchar_t;
use winapi::shared::basetsd::LONG_PTR;
use winapi::shared::minwindef::{FALSE, HINSTANCE, INT, LPARAM, LRESULT, TRUE, UINT, WPARAM};
use winapi::shared::ntdef::{LPCWSTR, LPWSTR, NULL, USHORT};
use winapi::shared::windef::{HMENU, HWND};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wincon::GetConsoleWindow;
use winapi::um::winuser::{
    CreateWindowExW, DestroyWindow, GetWindowLongPtrW, IsWindowVisible, RegisterClassExW,
    RegisterRawInputDevices, SetWindowLongPtrW, ShowWindow, UnregisterClassW, HWND_MESSAGE,
    RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE, SW_HIDE, WNDCLASSEXW,
};

pub struct Window<T> {
    _class: WindowClass<T>,
    hwnd: HWND,
    long_ptr_set: bool,
    _phantom: PhantomData<T>,
}

impl<T: WindowProc> Window<T> {
    pub fn new(proc: T) -> Result<Self> {
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
            NULL,
        ));

        let mut window = Window {
            _class: class,
            hwnd,
            long_ptr_set: false,
            _phantom: Default::default(),
        };

        unsafe {
            let console = GetConsoleWindow();
            if IsWindowVisible(console) == TRUE {
                ShowWindow(console, SW_HIDE);
            }
        }

        let mut proc = Box::new(proc);
        c_try_nonnull!(SetWindowLongPtrW(hwnd, 0, proc.as_mut() as *mut T as isize));
        std::mem::forget(proc);
        window.long_ptr_set = true;
        Ok(window)
    }
}

impl<T> Drop for Window<T> {
    fn drop(&mut self) {
        unsafe {
            if self.long_ptr_set {
                let ptr = window_proc_ptr_from_hwnd::<T>(self.hwnd);
                std::mem::drop(Box::from_raw(ptr));
            }
            assert_ne!(0, DestroyWindow(self.hwnd));
        }
    }
}

pub trait WindowProc {
    fn proc(&mut self, u_msg: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT;
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
    let this = window_proc_ptr_from_hwnd::<T>(hwnd);
    unsafe { (*this).proc(u_msg, w_param, l_param) }
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
        class.hInstance = c_try_nonnull!(GetModuleHandleW(NULL as LPWSTR));
        class.lpszClassName = name.as_ptr();

        let atom = c_try_nonnull!(RegisterClassExW(&class));

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
    pub fn new<T>(window: &Window<T>, usage_pages: &[USHORT]) -> Result<Self> {
        let mut devices = Vec::new();
        for usage_page in usage_pages {
            devices.push(RAWINPUTDEVICE {
                usUsagePage: *usage_page,
                usUsage: 1,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: window.hwnd,
            });
        }

        c_try!(RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as UINT,
            std::mem::size_of::<RAWINPUTDEVICE>() as UINT
        ));

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

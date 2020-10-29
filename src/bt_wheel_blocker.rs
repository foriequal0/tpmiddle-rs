use std::thread::{Builder as ThreadBuilder, JoinHandle};
use std::time::Duration;

use aligned::{Aligned, A8};
use anyhow::*;
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{DWORD, LPARAM, UINT, WPARAM};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::shared::windef::HWND;
use winapi::um::sysinfoapi::GetTickCount;
use winapi::um::winuser::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, PostMessageW, PostQuitMessage,
    LLMHF_INJECTED, LLMHF_LOWER_IL_INJECTED, MSLLHOOKSTRUCT, RAWINPUT, RAWINPUTDEVICELIST,
    RAWINPUTHEADER, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEMOUSE, RI_MOUSE_WHEEL, WM_INPUT,
    WM_MOUSEWHEEL, WM_USER,
};

use crate::hid::DeviceInfo;
use crate::hook::{HookProc, HookProcError, HookProcResult, LowLevelMouseHook};
use crate::util::ForceSendSync;
use crate::window::{Window, WindowProc, WindowProcError, WindowProcResult};

pub struct WheelBlocker {
    target_device_handle: HANDLE,
    hook_thread: HookThread,
}

impl WheelBlocker {
    pub fn new(target_device: &DeviceInfo) -> Result<Self> {
        let target_device_handle = get_target_device_handle(target_device)?;
        let hook_thread = HookThread::new()?;

        Ok(Self {
            target_device_handle,
            hook_thread,
        })
    }
}

impl WheelBlocker {
    pub fn peek_message(&mut self, u_msg: UINT, l_param: LPARAM) {
        if u_msg != WM_INPUT {
            return;
        }

        const SIZE: usize = std::mem::size_of::<RAWINPUT>();
        let mut raw_buffer: Aligned<A8, [u8; SIZE]> = Aligned([0; SIZE]);
        let (header, mouse) = unsafe {
            let mut size = SIZE as _;
            let result = GetRawInputData(
                l_param as _,
                RID_INPUT,
                raw_buffer.as_mut_ptr() as _,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as _,
            );
            if result == (-1i32 as _) {
                return;
            }
            let raw = raw_buffer.as_ptr() as *const RAWINPUT;
            let header = (*raw).header;
            if header.dwType != RIM_TYPEMOUSE {
                return;
            }
            (header, (*raw).data.mouse())
        };

        if mouse.usButtonFlags & RI_MOUSE_WHEEL == 0x00 {
            return;
        }

        if header.hDevice.is_null() {
            // Injected input.
            return;
        }

        self.hook_thread
            .block(header.hDevice == self.target_device_handle);
    }
}

fn get_target_device_handle(target_device: &DeviceInfo) -> Result<HANDLE> {
    const SIZE: UINT = std::mem::size_of::<RAWINPUTDEVICELIST>() as _;
    let mut num_devices = 0;
    c_try_ne!(
        -1i32 as _,
        GetRawInputDeviceList(NULL as _, &mut num_devices, SIZE)
    )?;
    let mut devices = vec![Default::default(); num_devices as _];
    c_try_ne!(
        -1i32 as _,
        GetRawInputDeviceList(devices.as_mut_ptr(), &mut num_devices, SIZE)
    )?;

    // TODO: Figure out why 02 is prefixed
    let target_device_vid_pid = format!(
        "VID&02{:04x}_PID&{:04x}",
        target_device.vendor_id, target_device.product_id
    );

    for device in devices {
        let mut size = 0;
        c_try_ne!(
            -1i32 as _,
            GetRawInputDeviceInfoW(device.hDevice, RIDI_DEVICENAME, NULL as _, &mut size)
        )?;
        let mut buffer = vec![0; size as _];
        c_try_ne!(
            -1i32 as _,
            GetRawInputDeviceInfoW(
                device.hDevice,
                RIDI_DEVICENAME,
                buffer.as_mut_ptr() as _,
                &mut size,
            )
        )?;
        let path = String::from_utf16(&buffer)?;
        if path.contains(&target_device_vid_pid) {
            return Ok(device.hDevice);
        }
    }

    Err(anyhow!("Device not found"))
}

struct BlockMessage {
    block: bool,
    time: DWORD,
}

struct HookThread {
    sender: Sender<BlockMessage>,
    hwnd: HWND,
    join_handle: Option<JoinHandle<()>>,
}

impl HookThread {
    fn new() -> Result<Self> {
        let (sender, receiver) = unbounded();
        let (hwnd_sender, hwnd_receiver) = bounded(1);
        let join_handle = ThreadBuilder::new()
            .name("WheelBlocker HookThread".to_owned())
            .spawn(move || {
                let window = Window::new("HookThreadWindow", HookThreadWindowProc)
                    .expect("Cannot create a window to hook");
                hwnd_sender
                    .send(ForceSendSync::new(window.hwnd))
                    .expect("Cannot send HWND to the caller");

                let _hook = LowLevelMouseHook::new(WheelBlockerHookProc { receiver })
                    .expect("Cannot install the hook");

                window
                    .run()
                    .expect("Error while running the message loop on the hook thread");
            })?;
        let hwnd = hwnd_receiver.recv()?;

        Ok(Self {
            sender,
            hwnd: *hwnd,
            join_handle: Some(join_handle),
        })
    }

    fn block(&self, block: bool) {
        self.sender
            .send(BlockMessage {
                block,
                time: unsafe { GetTickCount() },
            })
            .expect("Hook thread is dead");
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        c_try!(PostMessageW(self.hwnd, WM_USER_QUIT, 0, 0))
            .expect("Cannot send message to the hook thread");
        if let Some(join_handle) = self.join_handle.take() {
            join_handle.join().expect("Cannot join the hook thread");
        }
    }
}

struct WheelBlockerHookProc {
    receiver: Receiver<BlockMessage>,
}

impl HookProc for WheelBlockerHookProc {
    fn proc(&mut self, _n_code: c_int, w_param: WPARAM, l_param: LPARAM) -> HookProcResult {
        if w_param as UINT != WM_MOUSEWHEEL {
            return Err(HookProcError::UnhandledMessage);
        }

        let data = unsafe { *{ l_param as *mut MSLLHOOKSTRUCT } };
        if data.flags & LLMHF_INJECTED != 0x00 || data.flags & LLMHF_LOWER_IL_INJECTED != 0x00 {
            return Err(HookProcError::UnhandledMessage);
        }

        const MESSAGE_TIMEOUT_MS: u64 = 10;
        const DAY_MS: DWORD = 1000 * 60 * 60 * 24;
        while let Ok(message) = self
            .receiver
            .recv_timeout(Duration::from_millis(MESSAGE_TIMEOUT_MS))
        {
            let message_arrived_before: i32 =
                if data.time < DAY_MS && message.time >= DWORD::max_value() - DAY_MS {
                    // overflow && message arrived before data.
                    (DWORD::max_value() - message.time + data.time) as i32
                } else if message.time < DAY_MS && data.time >= DWORD::max_value() - DAY_MS {
                    // overflow && data arrived before message.
                    -((DWORD::max_value() - data.time + message.time) as i32)
                } else if message.time < data.time {
                    // non-overflow
                    (data.time - message.time) as i32
                } else {
                    // non-overflow
                    -((message.time - data.time) as i32)
                };

            if message_arrived_before < MESSAGE_TIMEOUT_MS as _ {
                return if message.block {
                    Ok(1)
                } else {
                    Err(HookProcError::UnhandledMessage)
                };
            } else {
                // discard too old message
            }
        }

        // Message from WheelBlocker didn't arrive in time.
        Err(HookProcError::UnhandledMessage)
    }
}

struct HookThreadWindowProc;

const WM_USER_QUIT: UINT = WM_USER + 1;

impl WindowProc for HookThreadWindowProc {
    fn proc(
        &mut self,
        _hwnd: HWND,
        u_msg: UINT,
        _w_param: WPARAM,
        _l_param: LPARAM,
    ) -> WindowProcResult {
        if u_msg == WM_USER_QUIT {
            unsafe { PostQuitMessage(0) };
            Ok(0)
        } else {
            Err(WindowProcError::UnhandledMessage)
        }
    }
}

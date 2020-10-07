use aligned::{Aligned, A8};
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{DWORD, FALSE, LPVOID, TRUE, UINT};
use winapi::shared::ntdef::USHORT;
use winapi::um::winuser::{
    BlockInput, GetRawInputData, GetRawInputDeviceInfoW, SendInput, HRAWINPUT, INPUT, INPUT_MOUSE,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, RAWINPUT,
    RAWINPUTHEADER, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_INPUT, RIM_TYPEHID,
};

const VID_DEVICE: DWORD = 0x17EF;
const PID_USB: DWORD = 0x60EE;
const PID_BT: DWORD = 0x60E1;

pub const USAGE_PAGES: [USHORT; 3] = [0xFF00, 0xFF10, 0xFFA0];

pub fn send_click(button: DWORD) {
    let mut input0: INPUT = Default::default();
    let mut input1: INPUT = Default::default();
    input0.type_ = INPUT_MOUSE;
    input1.type_ = INPUT_MOUSE;

    unsafe {
        let mi0 = input0.u.mi_mut();
        let mi1 = input1.u.mi_mut();

        if button == 3 {
            mi0.dwFlags = MOUSEEVENTF_MIDDLEDOWN;
            mi1.dwFlags = MOUSEEVENTF_MIDDLEUP;
        } else {
            mi0.dwFlags = MOUSEEVENTF_XDOWN;
            mi0.mouseData = button - 3;
            mi1.dwFlags = MOUSEEVENTF_XUP;
            mi1.mouseData = button - 3;
        }

        let mut input = [input0, input1];
        SendInput(
            input.len() as UINT,
            input.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as c_int,
        );
    }
}

pub fn send_wheel(event: DWORD, mouse_data: DWORD) {
    let mut input: INPUT = Default::default();

    unsafe {
        input.type_ = INPUT_MOUSE;
        let mi = input.u.mi_mut();
        mi.dwFlags = event;
        mi.mouseData = mouse_data;

        SendInput(1, &mut input, std::mem::size_of::<INPUT>() as c_int);
    }
}

#[derive(Debug)]
pub enum Event {
    ButtonDown,
    ButtonUp,
    Vertical(i8),
    Horizontal(i8),
}

#[derive(Eq, PartialEq)]
pub enum InputDevice {
    BT,
    USB,
}

pub struct Input {
    pub device: InputDevice,
    pub event: Event,
}

impl Input {
    pub(crate) fn from_raw_input(l_param: HRAWINPUT) -> Result<Self, ()> {
        const SIZE: usize = std::mem::size_of::<RAWINPUT>() + 9;
        let mut raw_buffer: Aligned<A8, [u8; SIZE]> = Aligned([0; SIZE]);
        let raw = unsafe {
            let mut size = SIZE as UINT;
            let result = GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                raw_buffer.as_mut_ptr() as LPVOID,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as UINT,
            );
            if result == (-1 as i32 as UINT) {
                return Err(());
            }
            raw_buffer.as_ptr() as *const RAWINPUT
        };

        let hid = unsafe {
            if (*raw).header.dwType != RIM_TYPEHID {
                return Err(());
            }

            (*raw).data.hid()
        };

        if hid.dwSizeHid < 3 || hid.dwCount != 1 {
            return Err(());
        }

        let device = unsafe {
            let mut device: RID_DEVICE_INFO = Default::default();
            let mut size = std::mem::size_of_val(&device) as UINT;
            device.cbSize = size;
            if GetRawInputDeviceInfoW(
                (*raw).header.hDevice,
                RIDI_DEVICEINFO,
                &mut device as *mut RID_DEVICE_INFO as LPVOID,
                &mut size,
            ) == -1 as i32 as UINT
            {
                return Err(());
            }
            device
        };

        if device.dwType != RIM_TYPEHID {
            return Err(());
        }

        let device_hid = unsafe { device.u.hid() };
        if device_hid.dwVendorId != VID_DEVICE
            || device_hid.dwProductId != PID_USB && device_hid.dwProductId != PID_BT
        {
            return Err(());
        }

        let device = if device_hid.dwProductId == PID_USB {
            InputDevice::USB
        } else {
            InputDevice::BT
        };

        let size = hid.dwSizeHid as usize * hid.dwCount as usize;
        if device_hid.dwProductId == PID_USB {
            debug_assert_eq!(size, 3);
        } else {
            debug_assert_eq!(size, 9);
        }
        let raw = unsafe { std::slice::from_raw_parts(hid.bRawData.as_ptr(), size) };

        let action = if raw[0] == 0x15 {
            if raw[2] & 0x04 != 0x00 {
                Event::ButtonDown
            } else {
                Event::ButtonUp
            }
        } else {
            let dx = raw[1] as i8;
            let dy = raw[2] as i8;
            if dx != 0 {
                Event::Horizontal(dx)
            } else if dy != 0 {
                Event::Vertical(dy)
            } else {
                unreachable!();
            }
        };

        Ok(Input {
            device,
            event: action,
        })
    }
}

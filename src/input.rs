use aligned::{Aligned, A8};
use anyhow::*;
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{DWORD, LPVOID, UINT};
use winapi::shared::ntdef::HANDLE;
use winapi::um::winuser::{
    GetRawInputData, GetRawInputDeviceInfoW, SendInput, HRAWINPUT, INPUT, INPUT_MOUSE,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, RAWINPUT,
    RAWINPUTHEADER, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_DEVICE_INFO_HID, RID_INPUT, RIM_TYPEHID,
};

use crate::hid::{DeviceInfo, PID_USB};

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

impl From<&RID_DEVICE_INFO_HID> for DeviceInfo {
    fn from(di: &RID_DEVICE_INFO_HID) -> Self {
        Self {
            vendor_id: di.dwVendorId as u16,
            product_id: di.dwProductId as u16,
            usage_page: di.usUsagePage as u16,
            usage: di.usUsage as u16,
        }
    }
}

impl Event {
    pub(crate) fn from_raw_input(
        l_param: HRAWINPUT,
        listening_device_infos: &'static [DeviceInfo],
    ) -> Result<Self, ()> {
        const SIZE: usize = std::mem::size_of::<RAWINPUT>() + 9;
        let mut raw_buffer: Aligned<A8, [u8; SIZE]> = Aligned([0; SIZE]);
        let (header, hid) = unsafe {
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
            let raw = raw_buffer.as_ptr() as *const RAWINPUT;
            ((*raw).header, (*raw).data.hid())
        };

        if header.dwType != RIM_TYPEHID {
            return Err(());
        }

        let device_info = get_device_info(header.hDevice).map_err(|_| ())?;
        if !listening_device_infos.iter().any(|x| *x == device_info) {
            return Err(());
        }

        assert_eq!(hid.dwCount, 1);
        let size = hid.dwSizeHid as usize;
        let raw = unsafe { std::slice::from_raw_parts(hid.bRawData.as_ptr(), size) };

        if raw[0] == 0x15 {
            if device_info.product_id == PID_USB {
                debug_assert_eq!(size, 3);
            } else {
                debug_assert!(size == 9);
            }
            if raw[2] & 0x04 != 0x00 {
                Ok(Event::ButtonDown)
            } else {
                Ok(Event::ButtonUp)
            }
        } else if raw[0] == 0x22 || raw[0] == 0x16 {
            debug_assert_eq!(size, 3);
            let dx = raw[1] as i8;
            let dy = raw[2] as i8;
            if dx != 0 {
                Ok(Event::Horizontal(dx))
            } else if dy != 0 {
                Ok(Event::Vertical(dy))
            } else {
                unreachable!();
            }
        } else {
            Err(())
        }
    }
}

pub fn get_device_info(handle: HANDLE) -> Result<DeviceInfo> {
    let mut rid_device_info: RID_DEVICE_INFO = Default::default();
    let mut size = std::mem::size_of_val(&rid_device_info) as UINT;
    rid_device_info.cbSize = size;
    c_try_ne!(
        (-1i32) as UINT,
        GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICEINFO,
            &mut rid_device_info as *mut RID_DEVICE_INFO as LPVOID,
            &mut size,
        )
    );

    ensure!(
        rid_device_info.dwType == RIM_TYPEHID,
        "Requested device is not HID"
    );

    let device_info = unsafe { DeviceInfo::from(rid_device_info.u.hid()) };
    Ok(device_info)
}

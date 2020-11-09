use anyhow::*;
use log::*;
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{DWORD, LPVOID, UINT};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::um::winuser::{
    GetRawInputData, GetRawInputDeviceInfoW, SendInput, HRAWINPUT, INPUT, INPUT_MOUSE,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, RAWHID,
    RAWINPUT, RAWINPUTHEADER, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_DEVICE_INFO_HID, RID_INPUT,
    RIM_TYPEHID,
};

use crate::hid::DeviceInfo;

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

pub struct EventReader<'a> {
    device_filter: &'a [DeviceInfo],
    buffer: Vec<u8>,
}

impl<'a> EventReader<'a> {
    pub fn new(device_filter: &'a [DeviceInfo]) -> Self {
        const HEADROOM: usize = 100;
        const SIZE: usize = std::mem::size_of::<RAWINPUT>() + HEADROOM;
        Self {
            device_filter,
            buffer: vec![0; SIZE],
        }
    }
}

impl<'a> EventReader<'a> {
    fn read_hid(&mut self, l_param: HRAWINPUT) -> Result<RawHID, ()> {
        unsafe {
            const SIZE: UINT = std::mem::size_of::<RAWINPUTHEADER>() as _;
            let mut size = 0;
            let result =
                GetRawInputData(l_param as HRAWINPUT, RID_INPUT, NULL as _, &mut size, SIZE);
            if result == (-1 as i32 as UINT) {
                return Err(());
            }

            if self.buffer.len() < size as usize {
                self.buffer.resize(size as usize, 0);
            }
            let result = GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                self.buffer.as_mut_ptr() as LPVOID,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as UINT,
            );
            if result == (-1 as i32 as UINT) {
                return Err(());
            }
            let raw = self.buffer.as_ptr() as *const RAWINPUT;
            let header = (*raw).header;
            if header.dwType != RIM_TYPEHID {
                return Err(());
            }

            let device_info = if let Ok(Some(device_info)) = get_hid_device_info(header.hDevice) {
                device_info
            } else {
                return Err(());
            };

            if !self.device_filter.iter().any(|x| *x == device_info) {
                return Err(());
            }

            Ok(RawHID::from((*raw).data.hid()))
        }
    }

    pub fn read_from_raw_input<'s>(
        &'s mut self,
        l_param: HRAWINPUT,
    ) -> Result<impl Iterator<Item = Event> + 's, ()> {
        let hid = self.read_hid(l_param)?;
        let result = hid.iter().filter_map(|packet| {
            if packet[0] == 0x15 {
                if packet[2] & 0x04 != 0x00 {
                    Some(Event::ButtonDown)
                } else {
                    Some(Event::ButtonUp)
                }
            } else if packet[0] == 0x22 || packet[0] == 0x16 {
                let dx = packet[1] as i8;
                let dy = packet[2] as i8;
                if dx != 0 {
                    Some(Event::Horizontal(dx))
                } else if dy != 0 {
                    Some(Event::Vertical(dy))
                } else {
                    warn!("Diagonal is unexpected");
                    None
                }
            } else {
                warn!("Unexpected packet ID: {:x}", packet[0]);
                None
            }
        });
        Ok(result)
    }
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

pub fn get_hid_device_info(handle: HANDLE) -> Result<Option<DeviceInfo>> {
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
    )?;

    if rid_device_info.dwType != RIM_TYPEHID {
        return Ok(None);
    }

    let device_info = unsafe { DeviceInfo::from(rid_device_info.u.hid()) };
    Ok(Some(device_info))
}

struct RawHID<'a> {
    size: usize,
    buffer: &'a [u8],
}

impl<'a> From<&'a RAWHID> for RawHID<'a> {
    fn from(hid: &'a RAWHID) -> Self {
        let size = hid.dwSizeHid as usize * hid.dwCount as usize;
        let buffer = unsafe { std::slice::from_raw_parts(hid.bRawData.as_ptr(), size) };
        Self {
            size: hid.dwSizeHid as _,
            buffer,
        }
    }
}

impl<'a> RawHID<'a> {
    fn iter(&self) -> std::slice::ChunksExact<'a, u8> {
        self.buffer.chunks_exact(self.size)
    }
}

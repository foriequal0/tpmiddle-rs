use crate::hid::DEVICE_INFO_MOUSE_BT;
use anyhow::*;
use winapi::shared::minwindef::{DWORD, LPVOID, UINT};
use winapi::shared::ntdef::NULL;
use winapi::um::winuser::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, PRAWINPUTDEVICELIST, RAWINPUTDEVICELIST,
    RIDI_DEVICEINFO, RIDI_DEVICENAME, RID_DEVICE_INFO, RIM_TYPEMOUSE,
};

pub fn get_mouse_id() -> Result<DWORD> {
    let identifier = format!(
        "VID_{:4X}&PID_{:4X}",
        DEVICE_INFO_MOUSE_BT.vendor_id, DEVICE_INFO_MOUSE_BT.product_id
    );

    for device in get_device_list()? {
        if device.dwType != RIM_TYPEMOUSE {
            continue;
        }
        let path = get_path(&device)?;
        if !path.contains(&identifier) {
            continue;
        }

        let device_info = get_device_info(&device)?;
        let mouse = unsafe {
            assert_eq!(device_info.dwType, RIM_TYPEMOUSE);
            device_info.u.mouse()
        };
        return Ok(mouse.dwId);
    }

    bail!("Mouse not found")
}

fn get_device_list() -> Result<Vec<RAWINPUTDEVICELIST>> {
    let size = std::mem::size_of::<RAWINPUTDEVICELIST>() as UINT;
    let mut num = 0;
    c_try_ne_unsafe!(
        (-1i32) as UINT,
        GetRawInputDeviceList(NULL as PRAWINPUTDEVICELIST, &mut num, size)
    );
    let mut devices = vec![Default::default(); num as usize];
    c_try_ne_unsafe!(
        (-1i32) as UINT,
        GetRawInputDeviceList(devices.as_mut_ptr(), &mut num, size)
    );

    Ok(devices)
}

fn get_path(device: &RAWINPUTDEVICELIST) -> Result<String> {
    let mut size = 0;
    c_try_ne_unsafe!(
        (-1i32) as UINT,
        GetRawInputDeviceInfoW(device.hDevice, RIDI_DEVICENAME, NULL as LPVOID, &mut size)
    );
    let mut buffer = vec![0; size as usize];
    c_try_ne_unsafe!(
        (-1i32) as UINT,
        GetRawInputDeviceInfoW(
            device.hDevice,
            RIDI_DEVICENAME,
            buffer.as_mut_ptr() as LPVOID,
            &mut size,
        )
    );

    Ok(String::from_utf16(&buffer)?)
}

fn get_device_info(device: &RAWINPUTDEVICELIST) -> Result<RID_DEVICE_INFO> {
    let mut device_info: RID_DEVICE_INFO = Default::default();
    let mut size = std::mem::size_of_val(&device_info) as UINT;
    device_info.cbSize = size;
    c_try_ne_unsafe!(
        (-1i32) as UINT,
        GetRawInputDeviceInfoW(
            device.hDevice,
            RIDI_DEVICEINFO,
            &mut device_info as *mut RID_DEVICE_INFO as LPVOID,
            &mut size,
        )
    );

    Ok(device_info)
}

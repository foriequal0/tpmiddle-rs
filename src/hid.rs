use std::ffi::{CStr, CString};
use std::fmt;
use std::str::FromStr;

use anyhow::*;
use hidapi::{HidApi, HidDevice, HidResult};
use thiserror::*;

pub const VID_LENOVO: u16 = 0x17EF;
pub const PID_USB: u16 = 0x60EE;
pub const PID_BT: u16 = 0x60E1;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

impl From<&hidapi::DeviceInfo> for DeviceInfo {
    fn from(di: &hidapi::DeviceInfo) -> Self {
        Self {
            vendor_id: di.vendor_id(),
            product_id: di.product_id(),
            usage_page: di.usage_page(),
            usage: di.usage(),
        }
    }
}

const DEVICE_INFO_SET_FEATURES_USB: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_USB,
    usage_page: 0x0C,
    usage: 0x01,
};

const DEVICE_INFO_SET_FEATURES_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: 0xFF01,
    usage: 0x01,
};

const DEVICE_INFO_MIDDLE_BUTTON_HID_USB: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_USB,
    usage_page: 0xFFA0,
    usage: 0x01,
};

const DEVICE_INFO_MIDDLE_BUTTON_HID_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: 0xFF00,
    usage: 0x01,
};

const DEVICE_INFO_WHEEL_HID_USB: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_USB,
    usage_page: 0xFF10,
    usage: 0x01,
};

const DEVICE_INFO_WHEEL_HID_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: 0xFF10,
    usage: 0x01,
};

const DEVICE_INFO_USB: &[DeviceInfo] =
    &[DEVICE_INFO_MIDDLE_BUTTON_HID_USB, DEVICE_INFO_WHEEL_HID_USB];
const DEVICE_INFO_BT: &[DeviceInfo] = &[DEVICE_INFO_MIDDLE_BUTTON_HID_BT, DEVICE_INFO_WHEEL_HID_BT];

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum ConnectionMethod {
    USB,
    BT,
}

impl ConnectionMethod {
    pub(crate) fn device_info(&self) -> &'static [DeviceInfo] {
        match self {
            Self::USB => DEVICE_INFO_USB,
            Self::BT => DEVICE_INFO_BT,
        }
    }
}

impl FromStr for ConnectionMethod {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "USB" | "usb" => Ok(ConnectionMethod::USB),
            "BT" | "bt" | "bluetooth" => Ok(ConnectionMethod::BT),
            _ => bail!("{} is not a valid connection method", s),
        }
    }
}

impl fmt::Display for ConnectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::USB => write!(f, "USB"),
            Self::BT => write!(f, "Bluetooth"),
        }
    }
}

#[derive(Error, Debug)]
pub enum SetFeaturesError {
    #[error("Hid error")]
    HidError(#[from] hidapi::HidError),
    #[error("Cannot find a keyboard")]
    CannotFindKeyboard(Option<ConnectionMethod>),
    #[error("Failed to set features")]
    CannotSetFeatures,
}

pub fn initialize_keyboard(
    connection_method: Option<ConnectionMethod>,
    sensitivity: Option<u8>,
    fn_lock: Option<bool>,
) -> Result<ConnectionMethod, SetFeaturesError> {
    let api = HidApi::new()?;

    let mut bt = None;
    let mut usb = None;

    for di in api.device_list() {
        let device_info = DeviceInfo::from(di);
        if device_info == DEVICE_INFO_SET_FEATURES_USB {
            usb = Some(Device {
                path: di.path().to_owned(),
                connection_method: ConnectionMethod::USB,
            });
        } else if device_info == DEVICE_INFO_SET_FEATURES_BT {
            bt = Some(Device {
                path: di.path().to_owned(),
                connection_method: ConnectionMethod::BT,
            });
        }
    }

    let device = match (connection_method, bt, usb) {
        (None, Some(bt), _) => bt,
        (None, None, Some(usb)) => usb,
        (Some(ConnectionMethod::BT), Some(bt), _) => bt,
        (Some(ConnectionMethod::USB), _, Some(usb)) => usb,
        _ => return Err(SetFeaturesError::CannotFindKeyboard(connection_method)),
    };

    println!(
        "Setting keyboard features over {}",
        device.connection_method
    );
    let result = if let ConnectionMethod::USB = device.connection_method {
        set_keyboard_features::<USB>(&api, &device.path, sensitivity, fn_lock)
    } else {
        set_keyboard_features::<BT>(&api, &device.path, sensitivity, fn_lock)
    };
    match result {
        Ok(_) => return Ok(device.connection_method),
        Err(err) => {
            eprintln!("Failed to initialize: {}", err);
        }
    };

    Err(SetFeaturesError::CannotSetFeatures)
}

struct Device {
    path: CString,
    connection_method: ConnectionMethod,
}

trait SetFeatures {
    fn set_sensitivity(device: &HidDevice, sensitivity: u8) -> HidResult<()>;
    fn set_fn_lock(device: &HidDevice, enable: bool) -> HidResult<()>;
    fn set_native_middle_button(device: &HidDevice, enable: bool) -> HidResult<()>;
}

fn set_keyboard_features<T: SetFeatures>(
    api: &HidApi,
    path: &CStr,
    sensitivity: Option<u8>,
    fn_lock: Option<bool>,
) -> Result<()> {
    let device = &api.open_path(path).context("Open device")?;

    if let Some(sensitivity) = sensitivity {
        T::set_sensitivity(&device, sensitivity).context("Set sensitivity")?;
    }
    if let Some(fn_lock) = fn_lock {
        T::set_fn_lock(&device, fn_lock).context("Set fn lock")?;
    }
    T::set_native_middle_button(&device, false).context("Set native middle button")?;
    Ok(())
}

struct USB;
impl SetFeatures for USB {
    fn set_sensitivity(device: &HidDevice, sensitivity: u8) -> HidResult<()> {
        assert!(sensitivity >= 1 && sensitivity <= 9);
        device.send_feature_report(&[0x13, 0x02, sensitivity, 0x00, 0x00, 0x00, 0x00, 0x00])
    }

    fn set_fn_lock(device: &HidDevice, enable: bool) -> HidResult<()> {
        let code = if enable { 0x01 } else { 0x00 };
        device.send_feature_report(&[0x13, 0x05, code, 0x00, 0x00, 0x00, 0x00, 0x00])
    }

    fn set_native_middle_button(device: &HidDevice, enable: bool) -> HidResult<()> {
        // 0x00: Keyboard sends scroll events
        // 0x01: "ThinkPad preferred scroll".
        let code = if enable { 0x00 } else { 0x01 };
        device.send_feature_report(&[0x13, 0x09, code, 0x00, 0x00, 0x00, 0x00, 0x00])
    }
}

struct BT;
impl SetFeatures for BT {
    fn set_sensitivity(device: &HidDevice, sensitivity: u8) -> HidResult<()> {
        assert!(sensitivity >= 1 && sensitivity <= 9);
        device.write(&[0x18, 0x02, sensitivity])?;
        Ok(())
    }

    fn set_fn_lock(device: &HidDevice, enable: bool) -> HidResult<()> {
        let code = if enable { 0x01 } else { 0x00 };
        device.write(&[0x18, 0x05, code])?;
        Ok(())
    }

    fn set_native_middle_button(device: &HidDevice, enable: bool) -> HidResult<()> {
        // 0x00: Keyboard sends scroll events
        // 0x01: "ThinkPad preferred scroll".
        let code = if enable { 0x00 } else { 0x01 };
        device.write(&[0x18, 0x09, code])?;
        Ok(())
    }
}

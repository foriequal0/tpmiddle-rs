use std::fmt;
use std::str::FromStr;

use hidapi::{HidApi, HidDevice, HidError, HidResult};
use log::*;
use thiserror::*;

pub const VID_LENOVO: u16 = 0x17EF;
pub const PID_USB: u16 = 0x60EE;
pub const PID_BT: u16 = 0x60E1;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct DeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

impl DeviceInfo {
    pub fn transport(&self) -> Option<Transport> {
        if self.product_id == PID_USB {
            Some(Transport::USB)
        } else if self.product_id == PID_BT {
            Some(Transport::BT)
        } else {
            None
        }
    }
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

impl std::fmt::Debug for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        struct UpperHex<T>(T);
        impl<T: std::fmt::UpperHex> std::fmt::Debug for UpperHex<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{:X}", self.0)
            }
        }

        f.debug_struct("DeviceInfo")
            .field("vendor_id", &UpperHex(self.vendor_id))
            .field("product_id", &UpperHex(self.product_id))
            .field("usage_page", &UpperHex(self.usage_page))
            .field("usage", &UpperHex(self.usage))
            .finish()
    }
}

const USAGE_PAGE_GENERIC_DESKTOP_CONTROL: u16 = 0x0001;
const USAGE_GENERIC_DESKTOP_CONTROL_MOUSE: u16 = 0x0002;

const USAGE_PAGE_CONSUMER_PAGE: u16 = 0x000C;
const USAGE_CONSUMER_PAGE_CONSUMER_CONTROL: u16 = 0x0001;

const USAGE_PAGE_VENDOR1: u16 = 0xFF00;
const USAGE_PAGE_VENDOR2: u16 = 0xFF10;
const USAGE_PAGE_VENDOR3: u16 = 0xFFA0;

const DEVICE_INFO_SET_FEATURES_USB: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_USB,
    usage_page: USAGE_PAGE_CONSUMER_PAGE,
    usage: USAGE_CONSUMER_PAGE_CONSUMER_CONTROL,
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
    usage_page: USAGE_PAGE_VENDOR3,
    usage: 0x01,
};

const DEVICE_INFO_MIDDLE_BUTTON_HID_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: USAGE_PAGE_VENDOR1,
    usage: 0x01,
};

const DEVICE_INFO_NON_NATIVE_WHEEL_USB: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_USB,
    usage_page: USAGE_PAGE_VENDOR2,
    usage: 0x01,
};

const DEVICE_INFO_NON_NATIVE_WHEEL_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: USAGE_PAGE_VENDOR2,
    usage: 0x01,
};

pub const DEVICE_INFO_WHEEL_HID_BT: DeviceInfo = DeviceInfo {
    vendor_id: VID_LENOVO,
    product_id: PID_BT,
    usage_page: USAGE_PAGE_GENERIC_DESKTOP_CONTROL,
    usage: USAGE_GENERIC_DESKTOP_CONTROL_MOUSE,
};

pub const DEVICE_INFOS_NOTIFY: &[DeviceInfo] = &[
    DEVICE_INFO_MIDDLE_BUTTON_HID_USB,
    DEVICE_INFO_MIDDLE_BUTTON_HID_BT,
    DEVICE_INFO_WHEEL_HID_BT,
];

pub const DEVICE_INFOS_SINK: &[DeviceInfo] = &[
    DEVICE_INFO_MIDDLE_BUTTON_HID_USB,
    DEVICE_INFO_NON_NATIVE_WHEEL_USB,
    DEVICE_INFO_MIDDLE_BUTTON_HID_BT,
    DEVICE_INFO_NON_NATIVE_WHEEL_BT,
    // sink to block
    DEVICE_INFO_WHEEL_HID_BT,
];

const DEVICE_INFO_USB: &[DeviceInfo] = &[
    DEVICE_INFO_MIDDLE_BUTTON_HID_USB,
    DEVICE_INFO_NON_NATIVE_WHEEL_USB,
];
const DEVICE_INFO_BT: &[DeviceInfo] = &[
    DEVICE_INFO_MIDDLE_BUTTON_HID_BT,
    DEVICE_INFO_NON_NATIVE_WHEEL_BT,
];

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum Transport {
    USB,
    BT,
}

impl Transport {
    pub fn device_info(&self) -> &'static [DeviceInfo] {
        match self {
            Self::USB => DEVICE_INFO_USB,
            Self::BT => DEVICE_INFO_BT,
        }
    }
}

#[derive(Error, Debug)]
pub enum TransportParseError {
    #[error("{0} is not a valid connection method")]
    InvalidConnectionMethod(String),
}

impl FromStr for Transport {
    type Err = TransportParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "USB" | "usb" => Ok(Transport::USB),
            "BT" | "bt" | "bluetooth" => Ok(Transport::BT),
            _ => Err(TransportParseError::InvalidConnectionMethod(s.to_owned())),
        }
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::USB => write!(f, "USB"),
            Self::BT => write!(f, "Bluetooth"),
        }
    }
}

#[derive(Error, Debug)]
pub enum InitializeError {
    #[error("Hid error: {0}")]
    HidError(#[from] hidapi::HidError),
    #[error("Cannot find a keyboard over {0}")]
    CannotFindKeyboard(Transport),
    #[error("Cannot set keyboard feature: {0}")]
    SetKeyboardFeatureError(#[source] SetKeyboardFeatureError),
}

pub fn initialize_keyboard(
    transport: Transport,
    sensitivity: Option<u8>,
    fn_lock: Option<bool>,
) -> Result<(), InitializeError> {
    let api = HidApi::new()?;

    for di in api.device_list() {
        let device_info = DeviceInfo::from(di);
        if transport == Transport::USB && device_info == DEVICE_INFO_SET_FEATURES_USB {
            match set_keyboard_features::<USB>(&di.open_device(&api)?, sensitivity, fn_lock) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    info!(
                        "Failed to set keyboard feature. path={path:?}, err={err:?}",
                        path = di.path(),
                        err = err
                    );
                }
            }
        } else if transport == Transport::BT && device_info == DEVICE_INFO_SET_FEATURES_BT {
            match set_keyboard_features::<BT>(&di.open_device(&api)?, sensitivity, fn_lock) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    info!(
                        "Failed to set keyboard feature. path={path:?}, err={err:?}",
                        path = di.path(),
                        err = err
                    );
                }
            }
        }
    }
    Err(InitializeError::CannotFindKeyboard(transport))
}

trait SetFeatures {
    fn set_sensitivity(device: &HidDevice, sensitivity: u8) -> HidResult<()>;
    fn set_fn_lock(device: &HidDevice, enable: bool) -> HidResult<()>;
    fn set_native_middle_button(device: &HidDevice, enable: bool) -> HidResult<()>;
}

#[derive(Error, Debug)]
pub enum SetKeyboardFeatureError {
    #[error("Cannot open device")]
    OpenDevice(#[source] HidError),
    #[error("Cannot set sensitivity")]
    SetSensitivity(#[source] HidError),
    #[error("Cannot set sensitivity")]
    SetFnLock(#[source] HidError),
    #[error("Cannot set sensitivity")]
    SetNativeMiddleButton(#[source] HidError),
}

fn set_keyboard_features<T: SetFeatures>(
    device: &HidDevice,
    sensitivity: Option<u8>,
    fn_lock: Option<bool>,
) -> Result<(), SetKeyboardFeatureError> {
    if let Some(sensitivity) = sensitivity {
        T::set_sensitivity(&device, sensitivity)
            .map_err(SetKeyboardFeatureError::SetSensitivity)?;
    }
    if let Some(fn_lock) = fn_lock {
        T::set_fn_lock(&device, fn_lock).map_err(SetKeyboardFeatureError::SetFnLock)?;
    }
    T::set_native_middle_button(&device, false)
        .map_err(SetKeyboardFeatureError::SetNativeMiddleButton)?;
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

use anyhow::*;
use hidapi::{HidApi, HidDevice, HidResult};

use crate::input::{PID_USB, VID_DEVICE};

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

pub fn set_keyboard_features(sensitivity: Option<u8>, fn_lock: Option<bool>) -> Result<()> {
    let api = HidApi::new()?;
    let mut is_sensitivity_set = sensitivity.is_none();
    let mut is_fn_lock_set = fn_lock.is_none();
    let mut is_native_middle_button_set = false;
    for di in api.device_list() {
        if di.vendor_id() != VID_DEVICE as u16 || di.product_id() != PID_USB as u16 {
            continue;
        }
        let device = di.open_device(&api)?;

        if let Some(sensitivity) = sensitivity {
            is_sensitivity_set |= set_sensitivity(&device, sensitivity).is_ok();
        }
        if let Some(fn_lock) = fn_lock {
            is_fn_lock_set |= set_fn_lock(&device, fn_lock).is_ok();
        }
        is_native_middle_button_set |= set_native_middle_button(&device, false).is_ok();
    }

    if is_sensitivity_set && is_fn_lock_set && is_native_middle_button_set {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to initialize TrackPoint Keyboard to preferred state"
        ))
    }
}

use winapi::ctypes::c_int;
use winapi::um::winuser::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_WHEEL,
};

use crate::mouse_hal::MouseHAL;
use crate::units::{Delta, Wheel};

pub struct MouseHALImpl;

impl MouseHAL for MouseHALImpl {
    fn send_middle_click() {
        let mut input0: INPUT = Default::default();
        let mut input1: INPUT = Default::default();
        input0.type_ = INPUT_MOUSE;
        input1.type_ = INPUT_MOUSE;

        unsafe {
            let mi0 = input0.u.mi_mut();
            let mi1 = input1.u.mi_mut();

            mi0.dwFlags = MOUSEEVENTF_MIDDLEDOWN;
            mi1.dwFlags = MOUSEEVENTF_MIDDLEUP;

            let mut input = [input0, input1];
            SendInput(
                input.len() as _,
                input.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as c_int,
            );
        }
    }

    fn send_wheel(delta: Wheel<Delta>) {
        let mut input: INPUT = Default::default();
        input.type_ = INPUT_MOUSE;

        unsafe {
            let mi = input.u.mi_mut();
            mi.dwFlags = match delta {
                Wheel::Vertical(_) => MOUSEEVENTF_WHEEL,
                Wheel::Horizontal(_) => MOUSEEVENTF_WHEEL,
            };
            mi.mouseData = delta.value().raw() as _;

            SendInput(1, &mut input, std::mem::size_of::<INPUT>() as _);
        }
    }
}

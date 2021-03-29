#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;
mod bt_wheel_blocker;
mod event_reader;
mod hook;
mod mouse_hal_impl;
mod tpmiddle;
mod transport_agnostic_tpmiddle;
mod window;

use anyhow::*;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;

use core::args::Args;
use core::hid::{DEVICE_INFOS_NOTIFY, DEVICE_INFOS_SINK};

use transport_agnostic_tpmiddle::TransportAgnosticTPMiddle;
use window::{hide_console, Devices, Window};

pub fn try_main(args: Args) -> Result<i32> {
    c_try!(SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS))?;

    let app = TransportAgnosticTPMiddle::new(args, DEVICE_INFOS_NOTIFY);
    let window = Window::new("MainWindow", app)?;
    let _devices = Devices::new(&window, &DEVICE_INFOS_NOTIFY, &DEVICE_INFOS_SINK)?;

    hide_console();
    window.run().map(|code| code as _)
}

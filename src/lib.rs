#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;
mod args;
mod bt_wheel_blocker;
mod control;
mod hid;
mod hook;
mod input;
mod middle_button_state;
mod mouse_hal;
mod mouse_hal_impl;
mod tpmiddle;
mod transport_agnostic_tpmiddle;
mod units;
mod window;

pub use args::Args;
pub use hid::{DEVICE_INFOS_NOTIFY, DEVICE_INFOS_SINK};
pub use transport_agnostic_tpmiddle::TransportAgnosticTPMiddle;
pub use window::{hide_console, Devices, Window};

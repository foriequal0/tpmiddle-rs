/// Import longer-name versions of macros only to not collide with legacy `log`
#[macro_use(o)]
extern crate slog;
#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;
mod bt_wheel_blocker;
mod control;
mod hid;
mod hook;
mod input;
mod tpmiddle;
mod window;

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs::OpenOptions;

use anyhow::*;
use clap::Clap;
use log::*;
use slog::{Drain, Duplicate, Logger, Never};
use slog_scope::GlobalLoggerGuard;
use winapi::shared::minwindef::{DWORD, LPARAM, UINT, WPARAM};
use winapi::shared::ntdef::HANDLE;
use winapi::shared::windef::HWND;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;
use winapi::um::winuser::{GIDC_ARRIVAL, GIDC_REMOVAL, WM_INPUT_DEVICE_CHANGE};

use crate::bt_wheel_blocker::WheelBlocker;
use crate::control::ScrollControlType;
use crate::hid::{
    DeviceInfo, Transport, DEVICE_INFOS_NOTIFY, DEVICE_INFOS_SINK, DEVICE_INFO_WHEEL_HID_BT,
};
use crate::input::get_device_info;
use crate::tpmiddle::TPMiddle;
use crate::window::{Devices, Window, WindowProc, WindowProcError, WindowProcResult};

enum ConnectionState {
    Disconnected,
    USB {
        tpmiddle: TPMiddle,
    },
    BT {
        wheel_blocker: Option<WheelBlocker>,
        tpmiddle: TPMiddle,
    },
}

struct TransportAgnosticTPMiddle<'a> {
    args: Args,
    notify_devices: &'a [DeviceInfo],
    devices: HashMap<HANDLE, DeviceInfo>,
    state: ConnectionState,
}

impl<'a> TransportAgnosticTPMiddle<'a> {
    fn new(args: Args, notify_devices: &'a [DeviceInfo]) -> Self {
        Self {
            args,
            notify_devices,
            devices: HashMap::new(),
            state: ConnectionState::Disconnected,
        }
    }

    fn try_connect_bt_then_usb(&mut self) {
        info!("Connecting");
        let bt_err = match self.connect_over(Transport::BT) {
            Ok(()) => {
                info!("Connected over {}!", Transport::BT);
                return;
            }
            Err(err) => err,
        };

        match self.connect_over(Transport::USB) {
            Ok(()) => {
                info!("Connected over {}!", Transport::USB);
            }
            Err(err) => {
                error!("Cannot connect over Bluetooth: {}", bt_err);
                error!("Cannot connect over USB: {}", err);
            }
        }
    }

    fn try_connect_over(&mut self, transport: Transport) {
        info!("Connecting over {}", transport);
        match self.connect_over(transport) {
            Ok(()) => {
                info!("Connected!");
            }
            Err(err) => {
                error!("Error: {}", err);
            }
        }
    }

    fn connect_over(&mut self, transport: Transport) -> Result<()> {
        hid::initialize_keyboard(transport, self.args.sensitivity, self.args.fn_lock())?;

        self.state = match transport {
            Transport::USB => {
                let tpmiddle =
                    TPMiddle::new(transport.device_info(), self.args.scroll.create_control());
                ConnectionState::USB { tpmiddle }
            }
            Transport::BT => {
                let (wheel_blocker, scroll) = if self.args.scroll == ScrollControlType::Smooth {
                    (
                        Some(WheelBlocker::new(&DEVICE_INFO_WHEEL_HID_BT)?),
                        ScrollControlType::Smooth,
                    )
                } else {
                    (None, ScrollControlType::ClassicHorizontalOnly)
                };

                let tpmiddle = TPMiddle::new(transport.device_info(), scroll.create_control());
                ConnectionState::BT {
                    wheel_blocker,
                    tpmiddle,
                }
            }
        };

        Ok(())
    }
}

impl<'a> WindowProc for TransportAgnosticTPMiddle<'a> {
    fn proc(
        &mut self,
        hwnd: HWND,
        u_msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> WindowProcResult {
        match u_msg {
            WM_INPUT_DEVICE_CHANGE if w_param as DWORD == GIDC_ARRIVAL => {
                let handle = l_param as HANDLE;
                trace!("ARRIVAL: {:?}", handle);
                let device_info = match get_device_info(handle) {
                    Ok(device_info) => device_info,
                    Err(err) => {
                        // The device is spuriously disconnected before handling this message
                        debug!("Error while get device info: {}", err);
                        return Ok(0);
                    }
                };
                debug!("ARRIVAL: {:?}, {:?}", device_info, device_info.transport());
                if !self.notify_devices.iter().any(|x| x == &device_info) {
                    return Ok(0);
                }

                self.devices.insert(handle, device_info);
                debug!("ARRIVAL: OK");

                if let ConnectionState::Disconnected = self.state {
                    self.try_connect_bt_then_usb();
                } else if matches!(self.state, ConnectionState::USB{..})
                    && device_info.transport() == Some(Transport::BT)
                {
                    // The wireless dongle is still connected, but the keyboard is changed to Bluetooth.
                    self.try_connect_over(Transport::BT);
                }
                Ok(0)
            }
            WM_INPUT_DEVICE_CHANGE if w_param as DWORD == GIDC_REMOVAL => {
                let handle = l_param as HANDLE;
                trace!("REMOVAL: {:?}", handle);
                if let Some(device_info) = self.devices.remove(&handle) {
                    debug!("REMOVAL: {:?}, {:?}", device_info, device_info.transport());

                    match (device_info.transport(), &self.state) {
                        (Some(Transport::BT), ConnectionState::BT { .. }) => {
                            self.state = ConnectionState::Disconnected;
                            info!("Disconnected: Bluetooth");
                            self.try_connect_over(Transport::USB);
                        }
                        (Some(Transport::USB), ConnectionState::USB { .. }) => {
                            self.state = ConnectionState::Disconnected;
                            info!("Disconnected: USB");
                            self.try_connect_over(Transport::BT);
                        }
                        _ => {}
                    }
                }

                Ok(0)
            }
            _ => match &mut self.state {
                ConnectionState::USB { tpmiddle } => tpmiddle.proc(hwnd, u_msg, w_param, l_param),
                ConnectionState::BT {
                    wheel_blocker,
                    tpmiddle,
                } => {
                    if let Some(wheel_blocker) = wheel_blocker {
                        wheel_blocker.peek_message(u_msg, l_param);
                    }
                    tpmiddle.proc(hwnd, u_msg, w_param, l_param)
                }
                _ => Err(WindowProcError::UnhandledMessage),
            },
        }
    }
}

#[derive(Clap)]
#[clap(version, about = "Tweak your TrackPoint Keyboard")]
pub struct Args {
    #[clap(short, long)]
    pub sensitivity: Option<u8>,

    #[clap(long)]
    pub fn_lock: bool,
    #[clap(long, hidden(true))]
    pub no_fn_lock: bool,

    #[clap(long, default_value = "classic")]
    pub scroll: ScrollControlType,

    #[clap(long)]
    pub log: Option<String>,
}

impl Args {
    fn fn_lock(&self) -> Option<bool> {
        match (self.fn_lock, self.no_fn_lock) {
            (true, true) => panic!(),
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        }
    }
}

fn set_logger(log: Option<&str>) -> Result<GlobalLoggerGuard> {
    let file_drain: Box<dyn slog::Drain<Ok = (), Err = Never> + Send> = if let Some(log) = log {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log)?;

        let decorator = slog_term::PlainSyncDecorator::new(file);

        Box::new(slog_term::CompactFormat::new(decorator).build().fuse())
    } else {
        Box::new(slog::Discard)
    };

    let stdout_drain = {
        let mut builder = slog_envlogger::LogBuilder::new(
            slog_term::CompactFormat::new(slog_term::TermDecorator::new().stdout().build()).build(),
        );

        if let Ok(s) = std::env::var("RUST_LOG") {
            builder = builder.parse(&s);
        } else {
            builder = builder.parse("info");
        }

        builder.build()
    };

    let drain = std::sync::Mutex::new(Duplicate::new(file_drain, stdout_drain).fuse()).fuse();

    let guard = slog_scope::set_global_logger(Logger::root(drain, o!()).into_erased());
    slog_stdlog::init()?;
    Ok(guard)
}

fn try_main(args: Args) -> Result<WPARAM> {
    c_try!(SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS))?;

    let app = TransportAgnosticTPMiddle::new(args, DEVICE_INFOS_NOTIFY);
    let window = Window::new("MainWindow", app)?;
    let _devices = Devices::new(&window, &DEVICE_INFOS_NOTIFY, &DEVICE_INFOS_SINK)?;

    window.run()
}

fn main() {
    let args: Args = Args::parse();
    if args.sensitivity.is_some() && args.sensitivity < Some(1) || args.sensitivity > Some(9) {
        eprintln!("Argument error: --sensitivity value should be in [1, 9]");
        std::process::exit(-1);
    }
    if args.fn_lock && args.no_fn_lock {
        eprintln!("Argument error: Flag 'fn-lock' and 'no-fn-lock' cannot be used simultaneously",);
        std::process::exit(-1);
    }

    let _logger =
        set_logger(args.log.as_ref().map(Borrow::borrow)).expect("Error: Cannot install logger");

    std::panic::set_hook(Box::new(|info| error!("Error: {:?}", info)));

    match try_main(args) {
        Ok(code) => std::process::exit(code as i32),
        Err(err) => {
            error!("Error: {:?}", err);
            std::process::exit(-1);
        }
    }
}

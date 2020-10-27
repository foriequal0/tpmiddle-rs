/// Import longer-name versions of macros only to not collide with legacy `log`
#[macro_use(o)]
extern crate slog;

#[macro_use]
mod util;
mod control;
mod hid;
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
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::shared::windef::HWND;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;
use winapi::um::winuser::{
    DispatchMessageW, GetMessageW, TranslateMessage, GIDC_ARRIVAL, GIDC_REMOVAL, MSG,
    WM_INPUT_DEVICE_CHANGE,
};

use crate::control::ScrollControlType;
use crate::hid::{DeviceInfo, InitializeError, Transport};
use crate::input::get_device_info;
use crate::tpmiddle::TPMiddle;
use crate::window::{Devices, Window, WindowProc, WindowProcError, WindowProcResult};

enum ConnectionState {
    Disconnected,
    USB { tpmiddle: TPMiddle },
    BT { tpmiddle: TPMiddle },
}

struct TransportAgnosticTPMiddle {
    args: Args,
    devices: HashMap<HANDLE, DeviceInfo>,
    state: ConnectionState,
}

impl TransportAgnosticTPMiddle {
    fn new(args: Args) -> Self {
        Self {
            args,
            devices: HashMap::new(),
            state: ConnectionState::Disconnected,
        }
    }
}

impl TransportAgnosticTPMiddle {
    fn try_connect_bt_then_usb(&mut self) {
        info!("Connecting");
        let bt_err = match self.connect_over(Transport::BT) {
            Ok(warning) => {
                info!("Connected over {}!", Transport::BT);
                if let Some(warning) = warning {
                    warn!("{}", warning);
                }
                return;
            }
            Err(err) => err,
        };

        match self.connect_over(Transport::USB) {
            Ok(warning) => {
                info!("Connected over {}!", Transport::USB);
                if let Some(warning) = warning {
                    warn!("{}", warning);
                }
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
            Ok(warning) => {
                info!("Connected!");
                if let Some(warning) = warning {
                    warn!("{}", warning);
                }
            }
            Err(err) => {
                error!("Error: {}", err);
            }
        }
    }

    fn connect_over(
        &mut self,
        transport: Transport,
    ) -> Result<Option<&'static str>, InitializeError> {
        let mut warning = None;
        hid::initialize_keyboard(transport, self.args.sensitivity, self.args.fn_lock())?;

        let scroll = if let Transport::BT = transport {
            if let ScrollControlType::Smooth = self.args.scroll {
                warning = Some("Warning: Smooth scroll is not available over Bluetooth");
            }
            ScrollControlType::ClassicHorizontalOnly
        } else {
            self.args.scroll
        };

        let tpmiddle = TPMiddle::new(transport.device_info(), scroll.create_control());
        self.state = match transport {
            Transport::USB => ConnectionState::USB { tpmiddle },
            Transport::BT => ConnectionState::BT { tpmiddle },
        };

        Ok(warning)
    }
}

impl WindowProc for TransportAgnosticTPMiddle {
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
                self.devices.insert(handle, device_info);
                debug!("ARRIVAL: {:?}, {:?}", device_info, device_info.transport());

                if let ConnectionState::Disconnected = self.state {
                    self.try_connect_bt_then_usb();
                } else if matches!(self.state, ConnectionState::USB{..})
                    && device_info.transport() == Transport::BT
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
                        (Transport::BT, ConnectionState::BT { .. }) => {
                            self.state = ConnectionState::Disconnected;
                            info!("Disconnected: Bluetooth");
                            self.try_connect_over(Transport::USB);
                        }
                        (Transport::USB, ConnectionState::USB { .. }) => {
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
                ConnectionState::BT { tpmiddle } | ConnectionState::USB { tpmiddle } => {
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

fn try_main() -> Result<WPARAM> {
    let args: Args = Args::parse();
    if args.sensitivity.is_some() && args.sensitivity < Some(1) || args.sensitivity > Some(9) {
        bail!("--sensitivity value should be in [1, 9]");
    }
    if args.fn_lock && args.no_fn_lock {
        bail!("Error: Flag 'fn-lock' and 'no-fn-lock' cannot be used simultaneously",);
    }

    let _logger = set_logger(args.log.as_ref().map(Borrow::borrow))?;

    c_try!(SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS))?;

    let app = TransportAgnosticTPMiddle::new(args);
    let window = Window::new(app)?;
    let _devices = Devices::new(&window)?;

    let exit_code = unsafe {
        let mut message: MSG = Default::default();
        loop {
            let status = c_try_ne_unsafe!(-1, GetMessageW(&mut message, NULL as HWND, 0, 0))?;
            if status == 0 {
                break message.wParam;
            }

            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    Ok(exit_code)
}

fn main() {
    match try_main() {
        Ok(code) => std::process::exit(code as i32),
        Err(err) => {
            error!("Error: {:?}", err);
            std::process::exit(-1);
        }
    }
}

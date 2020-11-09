use std::collections::HashMap;

use anyhow::*;
use log::*;
use winapi::shared::minwindef::{DWORD, LPARAM, UINT, WPARAM};
use winapi::shared::ntdef::HANDLE;
use winapi::shared::windef::HWND;
use winapi::um::winuser::{GIDC_ARRIVAL, GIDC_REMOVAL, WM_INPUT_DEVICE_CHANGE};

use crate::args::Args;
use crate::bt_wheel_blocker::WheelBlocker;
use crate::hid;
use crate::hid::{DeviceInfo, Transport, PID_BT, VID_LENOVO};
use crate::input::get_hid_device_info;
use crate::tpmiddle::TPMiddle;
use crate::window::{WindowProc, WindowProcError, WindowProcResult};

enum ConnectionState {
    Disconnected,
    USB {
        tpmiddle: TPMiddle,
    },
    BT {
        wheel_blocker: WheelBlocker,
        tpmiddle: TPMiddle,
    },
}

pub struct TransportAgnosticTPMiddle<'a> {
    args: Args,
    notify_devices: &'a [DeviceInfo],
    devices: HashMap<HANDLE, DeviceInfo>,
    state: ConnectionState,
}

impl<'a> TransportAgnosticTPMiddle<'a> {
    pub fn new(args: Args, notify_devices: &'a [DeviceInfo]) -> Self {
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
                let wheel_blocker = WheelBlocker::new(VID_LENOVO, PID_BT)?;
                let tpmiddle =
                    TPMiddle::new(transport.device_info(), self.args.scroll.create_control());
                ConnectionState::BT {
                    wheel_blocker,
                    tpmiddle,
                }
            }
        };

        Ok(())
    }

    fn on_mouse_device_change(&mut self) -> Result<()> {
        if let ConnectionState::BT { wheel_blocker, .. } = &mut self.state {
            wheel_blocker.rescan_target_device_handle()?;
        }
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
                let handle = l_param as _;
                trace!("ARRIVAL: {:?}", handle);

                match get_hid_device_info(handle) {
                    Ok(Some(device_info)) => {
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
                    }
                    Ok(None) => {
                        self.on_mouse_device_change()?;
                    }
                    Err(err) => {
                        // The device is spuriously disconnected before handling this message
                        debug!("Error while get device info: {}", err);
                        return Ok(0);
                    }
                };

                Ok(0)
            }
            WM_INPUT_DEVICE_CHANGE if w_param as DWORD == GIDC_REMOVAL => {
                let handle = l_param as _;
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
                    wheel_blocker.peek_message(u_msg, l_param);
                    tpmiddle.proc(hwnd, u_msg, w_param, l_param)
                }
                _ => Err(WindowProcError::UnhandledMessage),
            },
        }
    }
}

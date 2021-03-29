use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::winuser::{HRAWINPUT, WM_INPUT};

use core::control::ScrollControl;
use core::hid::DeviceInfo;
use core::middle_button::{MiddleButtonEvent, MiddleButtonState};
use core::units::{Tick, Wheel};

use crate::event_reader::EventReader;
use crate::window::{WindowProc, WindowProcError, WindowProcResult};

pub struct TPMiddle {
    middle_button_state: MiddleButtonState,
    control: Box<dyn ScrollControl>,
    event_reader: EventReader<'static>,
}

impl TPMiddle {
    pub fn new(device_filter: &'static [DeviceInfo], control: Box<dyn ScrollControl>) -> Self {
        TPMiddle {
            middle_button_state: MiddleButtonState::Idle,
            control,
            event_reader: EventReader::new(device_filter),
        }
    }
}

impl WindowProc for TPMiddle {
    fn proc(
        &mut self,
        _hwnd: HWND,
        u_msg: UINT,
        _w_param: WPARAM,
        l_param: LPARAM,
    ) -> WindowProcResult {
        if u_msg != WM_INPUT {
            return Err(WindowProcError::UnhandledMessage);
        }

        let events = if let Ok(events) = self.event_reader.read_from_raw_input(l_param as HRAWINPUT)
        {
            events
        } else {
            return Ok(0);
        };

        for event in events {
            match event {
                MiddleButtonEvent::ButtonDown => self.middle_button_state.down(),
                MiddleButtonEvent::ButtonUp => {
                    self.control.stop();
                    if self.middle_button_state.up() {
                        self.control.middle_click();
                    }
                }
                MiddleButtonEvent::Vertical(dy) => {
                    self.middle_button_state.scroll();
                    self.control.tick(Wheel::Vertical(Tick::from_raw(dy)));
                }
                MiddleButtonEvent::Horizontal(dx) => {
                    self.middle_button_state.scroll();
                    self.control.tick(Wheel::Horizontal(Tick::from_raw(dx)));
                }
            }
        }

        Ok(0)
    }
}

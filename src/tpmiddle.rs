use std::time::{Duration, Instant};

use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::winuser::{HRAWINPUT, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, WM_INPUT};

use crate::control::ScrollControl;
use crate::hid::DeviceInfo;
use crate::input::{send_click, Event, EventReader};
use crate::window::{WindowProc, WindowProcError, WindowProcResult};

const MAX_MIDDLE_CLICK_DURATION: Duration = Duration::from_millis(50);

enum State {
    MiddleUp,
    MiddleDown { time: Instant },
    Scroll,
}

pub struct TPMiddle {
    state: State,
    control: Box<dyn ScrollControl>,
    event_reader: EventReader<'static>,
}

impl TPMiddle {
    pub fn new(device_filter: &'static [DeviceInfo], control: Box<dyn ScrollControl>) -> Self {
        TPMiddle {
            state: State::MiddleUp,
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

        let event = if let Ok(event) = self.event_reader.read_from_raw_input(l_param as HRAWINPUT) {
            event
        } else {
            return Ok(0);
        };

        match event {
            Event::ButtonDown => {
                self.state = State::MiddleDown {
                    time: Instant::now(),
                };
            }
            Event::ButtonUp => {
                self.control.stop();
                if let State::MiddleDown { time } = self.state {
                    let now = Instant::now();
                    if now <= time + MAX_MIDDLE_CLICK_DURATION {
                        send_click(3);
                    }
                }
                self.state = State::MiddleUp;
            }
            Event::Vertical(dy) => {
                self.control.scroll(MOUSEEVENTF_WHEEL, dy);
            }
            Event::Horizontal(dx) => {
                self.state = State::Scroll;
                self.control.scroll(MOUSEEVENTF_HWHEEL, dx);
            }
        }

        Ok(0)
    }
}

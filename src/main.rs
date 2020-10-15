#[macro_use]
mod util;
mod control;
mod hid;
mod input;
mod window;

use std::time::{Duration, Instant};

use anyhow::*;
use clap::Clap;
use winapi::shared::minwindef::{LPARAM, LRESULT, TRUE, UINT, WPARAM};
use winapi::shared::ntdef::NULL;
use winapi::shared::windef::HWND;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;
use winapi::um::winuser::{
    DispatchMessageW, GetMessageW, TranslateMessage, HRAWINPUT, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_WHEEL, MSG, WM_INPUT, WM_NCCREATE,
};

use crate::control::{ScrollControl, ScrollControlType};
use crate::input::{send_click, Event, Input, USAGE_PAGES};
use crate::window::{Devices, Window, WindowProc};

const MAX_MIDDLE_CLICK_DURATION: Duration = Duration::from_millis(50);

enum State {
    MiddleUp,
    MiddleDown { time: Instant },
    Scroll,
}

struct TPMiddle {
    state: State,
    control: Box<dyn ScrollControl>,
}

impl TPMiddle {
    fn new(control: Box<dyn ScrollControl>) -> Result<Self> {
        Ok(TPMiddle {
            state: State::MiddleUp,
            control,
        })
    }
}

impl WindowProc for TPMiddle {
    fn proc(&mut self, u_msg: UINT, _w_param: WPARAM, l_param: LPARAM) -> LRESULT {
        if u_msg != WM_INPUT {
            return match u_msg {
                WM_NCCREATE => TRUE as LRESULT,
                _ => 0 as LRESULT,
            };
        }

        let input = if let Ok(input) = Input::from_raw_input(l_param as HRAWINPUT) {
            input
        } else {
            return 0;
        };

        match input.event {
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

        0
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
}

impl Args {
    fn fn_lock(&self) -> Result<Option<bool>> {
        match (self.fn_lock, self.no_fn_lock) {
            (true, true) => {
                bail!("Error: Flag 'fn-lock' and 'no-fn-lock' cannot be used simultaneously",);
            }
            (true, _) => Ok(Some(true)),
            (_, true) => Ok(Some(false)),
            _ => Ok(None),
        }
    }
}

fn try_main() -> Result<WPARAM> {
    let args: Args = Args::parse();
    if args.sensitivity.is_some() && args.sensitivity < Some(1) || args.sensitivity > Some(9) {
        bail!("--sensitivity value should be in [1, 9]");
    }

    println!("Initializing...");
    hid::set_keyboard_features(args.sensitivity, args.fn_lock()?)?;

    c_try!(SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS));

    let app = TPMiddle::new(args.scroll.create_control())?;
    let window = Window::new(app)?;
    let _devices = Devices::new(&window, &USAGE_PAGES)?;

    println!("Started!");
    let exit_code = unsafe {
        let mut message: MSG = Default::default();
        loop {
            let status = c_try_ne!(-1, GetMessageW(&mut message, NULL as HWND, 0, 0));
            if status == 0 {
                break message.wParam;
            }

            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    Ok(exit_code)
}

fn main() -> Result<()> {
    let code = try_main()?;
    std::process::exit(code as i32);
}

#[macro_use]
mod util;
mod control;
mod hid;
mod input;
mod tpmiddle;
mod window;

use std::time::Duration;

use anyhow::*;
use clap::Clap;
use winapi::shared::minwindef::WPARAM;
use winapi::shared::ntdef::NULL;
use winapi::shared::windef::HWND;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;
use winapi::um::winuser::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

use crate::control::ScrollControlType;
use crate::hid::Transport;
use crate::tpmiddle::TPMiddle;
use crate::window::{Devices, Window};

const MAX_MIDDLE_CLICK_DURATION: Duration = Duration::from_millis(50);

#[derive(Clap)]
#[clap(version, about = "Tweak your TrackPoint Keyboard")]
pub struct Args {
    #[clap(long)]
    pub connection: Option<Transport>,

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
    c_try!(SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS));

    let mut args: Args = Args::parse();
    if args.sensitivity.is_some() && args.sensitivity < Some(1) || args.sensitivity > Some(9) {
        bail!("--sensitivity value should be in [1, 9]");
    }

    let transport = hid::initialize_keyboard(args.connection, args.sensitivity, args.fn_lock()?)?;

    if let Transport::BT = transport {
        if args.scroll == ScrollControlType::Smooth {
            eprintln!("Smooth scroll over Bluetooth is not supported");
        }
        args.scroll = ScrollControlType::ClassicHorizontalOnly
    }

    let listening_device_infos = transport.device_info();
    let app = TPMiddle::new(listening_device_infos, args.scroll.create_control())?;
    let window = Window::new(app)?;
    let _devices = Devices::new(&window, listening_device_infos)?;

    println!("Started!");
    let exit_code = unsafe {
        let mut message: MSG = Default::default();
        loop {
            let status = c_try_ne_unsafe!(-1, GetMessageW(&mut message, NULL as HWND, 0, 0));
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

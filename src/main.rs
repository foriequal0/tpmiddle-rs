/// Import longer-name versions of macros only to not collide with legacy `log`
#[macro_use(o)]
extern crate slog;

use std::borrow::Borrow;
use std::fs::OpenOptions;

use anyhow::*;
use clap::Clap;
use log::*;
use slog::{Drain, Duplicate, Logger, Never};
use slog_scope::GlobalLoggerGuard;
use winapi::shared::minwindef::WPARAM;
use winapi::um::processthreadsapi::{GetCurrentProcess, SetPriorityClass};
use winapi::um::winbase::HIGH_PRIORITY_CLASS;

use tpmiddle_rs::{
    c_try, hide_console, Args, Devices, TransportAgnosticTPMiddle, Window, DEVICE_INFOS_NOTIFY,
    DEVICE_INFOS_SINK,
};

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

    hide_console();
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

use clap::Clap;

use crate::control::ScrollControlType;

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
    pub fn fn_lock(&self) -> Option<bool> {
        match (self.fn_lock, self.no_fn_lock) {
            (true, true) => panic!(),
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        }
    }
}

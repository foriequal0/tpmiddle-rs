use std::str::FromStr;

use anyhow::*;
use winapi::shared::minwindef::DWORD;
use winapi::um::winuser::WHEEL_DELTA;

use crate::input::send_wheel;

pub enum ScrollControlType {
    Classic,
}

impl FromStr for ScrollControlType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "classic" => Ok(ScrollControlType::Classic),
            _ => Err(anyhow!("`{}` is an invalid type", s)),
        }
    }
}

impl ScrollControlType {
    pub(crate) fn create_control(&self) -> Box<dyn ScrollControl> {
        match self {
            ScrollControlType::Classic => Box::new(ClassicController),
        }
    }
}

pub trait ScrollControl {
    fn scroll(&self, event: DWORD, units: i8);
    fn stop(&self);
}

struct ClassicController;

impl ScrollControl for ClassicController {
    fn scroll(&self, event: DWORD, delta: i8) {
        send_wheel(event, (delta as i32 * WHEEL_DELTA as i32) as DWORD)
    }

    fn stop(&self) {}
}

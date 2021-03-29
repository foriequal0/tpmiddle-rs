use std::time::{Duration, Instant};

const MAX_MIDDLE_CLICK_DURATION: Duration = Duration::from_millis(500);

pub enum MiddleButtonState {
    Idle,
    MiddleDown { time: Instant },
    Scroll,
}

impl MiddleButtonState {
    pub fn down(&mut self) {
        *self = MiddleButtonState::MiddleDown {
            time: Instant::now(),
        };
    }

    pub fn up(&mut self) -> bool {
        if let MiddleButtonState::MiddleDown { time } = self {
            let now = Instant::now();
            if now <= *time + MAX_MIDDLE_CLICK_DURATION {
                return true;
            }
        }
        *self = MiddleButtonState::Idle;
        false
    }

    pub fn scroll(&mut self) {
        *self = MiddleButtonState::Scroll
    }
}

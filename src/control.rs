use std::str::FromStr;

use anyhow::*;
use winapi::shared::minwindef::DWORD;
use winapi::um::winuser::WHEEL_DELTA;

use crate::input::send_wheel;

pub enum ScrollControlType {
    Classic,
    Smooth,
}

impl FromStr for ScrollControlType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "classic" => Ok(ScrollControlType::Classic),
            "smooth" => Ok(ScrollControlType::Smooth),
            _ => Err(anyhow!("`{}` is an invalid type", s)),
        }
    }
}

impl ScrollControlType {
    pub(crate) fn create_control(&self) -> Box<dyn ScrollControl> {
        match self {
            ScrollControlType::Classic => Box::new(classic::ClassicController),
            ScrollControlType::Smooth => Box::new(smooth::SmoothController::new()),
        }
    }
}

pub trait ScrollControl {
    fn scroll(&self, event: DWORD, units: i8);
    fn stop(&self);
}

mod classic {
    use super::*;

    pub struct ClassicController;

    impl ScrollControl for ClassicController {
        fn scroll(&self, event: DWORD, delta: i8) {
            send_wheel(event, (delta as i32 * WHEEL_DELTA as i32) as DWORD)
        }

        fn stop(&self) {}
    }
}

mod smooth {
    use super::*;

    use std::thread::{spawn, JoinHandle};
    use std::time::Duration;

    use crossbeam_channel::{bounded, never, tick, Sender};

    const WHEEL_TICK_FREQ: u64 = 60;
    const WHEEL_TICK_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / WHEEL_TICK_FREQ);

    const BUFFER_MOVING_AVG: f32 = 0.1;
    const EXPONENTIAL_DECAY: f32 = 0.1;

    pub struct SmoothController {
        sender: Option<Sender<Event>>,
        join_handle: Option<JoinHandle<()>>,
    }

    impl SmoothController {
        pub fn new() -> Self {
            let (sender, receiver) = bounded(1);
            let mut state = State::Nop;
            let mut wheel_tick = never();
            let join_handle = spawn(move || loop {
                crossbeam_channel::select! {
                    recv(wheel_tick) -> _ => {}
                    recv(receiver) -> event => {
                        match event {
                            Ok(Event::Scroll { event, delta }) => {
                                if state.feed(event, delta) {
                                    wheel_tick = tick(WHEEL_TICK_INTERVAL);
                                }
                            }
                            Ok(Event::Stop) => {
                                state = State::Nop;
                                wheel_tick = never();
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                };

                if let Some(wheel) = state.tick() {
                    send_wheel(wheel.event, wheel.mouse_data);
                } else {
                    wheel_tick = never();
                }
            });
            Self {
                sender: Some(sender),
                join_handle: Some(join_handle),
            }
        }
    }

    impl ScrollControl for SmoothController {
        fn scroll(&self, event: DWORD, delta: i8) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(Event::Scroll { event, delta })
                .expect("Smooth scrolling thread is dead")
        }

        fn stop(&self) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(Event::Stop)
                .expect("Smooth scrolling thread is dead")
        }
    }

    impl Drop for SmoothController {
        fn drop(&mut self) {
            std::mem::drop(self.sender.take());
            if let Some(join_handle) = self.join_handle.take() {
                join_handle.join().expect("Smooth scrolling thread is dead");
            }
        }
    }

    enum Event {
        Scroll { event: DWORD, delta: i8 },
        Stop,
    }

    #[derive(Debug)]
    enum State {
        Scrolling {
            event: DWORD,
            buffer: f32,
            reservoir: f32,
            error: f32,
        },
        Nop,
    }

    impl State {
        fn feed(&mut self, event: DWORD, delta: i8) -> bool {
            // Empirical feed pattern (number is `delta`)
            // slow scroll  : 1     1     1... >= 100ms interval, up to few seconds.
            // normal scroll: 1  1  1  1  1... <  100ms interval.
            // fast scroll  : 3333333333333... ~= 15ms interval, with greater `delta`
            match self {
                State::Scrolling {
                    event: prev_event,
                    buffer,
                    reservoir,
                    ..
                } if *prev_event == event && reservoir.signum() as i8 == delta.signum() => {
                    *buffer += delta as f32;
                    false
                }
                _ => {
                    *self = State::Scrolling {
                        event,
                        buffer: delta as f32,
                        reservoir: 0.0,
                        error: 0.0,
                    };
                    true
                }
            }
        }

        fn tick(&mut self) -> Option<WheelTick> {
            match *self {
                State::Scrolling {
                    event,
                    ref mut buffer,
                    ref mut reservoir,
                    ref mut error,
                } => {
                    let x = *buffer * BUFFER_MOVING_AVG;
                    *buffer -= x;
                    *reservoir += x;

                    let amount = *reservoir * EXPONENTIAL_DECAY;

                    let mouse_data = {
                        let delta_f32 = amount * WHEEL_DELTA as f32;
                        let mut delta = delta_f32 as i32;
                        *error += delta_f32 - delta as f32;
                        if *reservoir > 0.0 {
                            while *error >= 1.0 as f32 {
                                *error -= 1.0 as f32;
                                delta += 1;
                            }
                        } else if *reservoir < 0.0 {
                            while *error <= -1.0 as f32 {
                                *error += 1.0 as f32;
                                delta -= 1;
                            }
                        }
                        delta as DWORD
                    };

                    *reservoir -= amount;
                    if reservoir.abs() < 1.0 / WHEEL_DELTA as f32 {
                        *self = State::Nop
                    }

                    Some(WheelTick { event, mouse_data })
                }
                State::Nop => None,
            }
        }
    }

    struct WheelTick {
        event: DWORD,
        mouse_data: DWORD,
    }
}

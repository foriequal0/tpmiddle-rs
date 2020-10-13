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
    use std::time::{Duration, Instant};

    use crossbeam_channel::{bounded, never, tick, Sender};

    // Empirically found min feed interval
    const MIN_FEED_INTERVAL_SECS: f32 = 0.015;

    const WHEEL_TICK_FREQ: u64 = 120;
    const WHEEL_TICK_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / WHEEL_TICK_FREQ);
    const WHEEL_TICK_INTERVAL_SECS: f32 = 1.0 / WHEEL_TICK_FREQ as f32;

    /// Time to fully drain the buffer into the reservoir.
    const BUFFER_MAX_DRAIN_DURATION_SECS: f32 = 0.05;

    /// Time to stop scrolling.
    const LINEAR_DECAY_DURATION_SECS: f32 = 0.3;

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
                                let now = Instant::now();
                                if state.feed(now, event, delta) {
                                    wheel_tick = tick(WHEEL_TICK_INTERVAL);
                                }
                            }
                            Ok(Event::Stop) => {
                                state =  State::Nop;
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
            scroll_direction: f32,
            buffer: f32,
            decay: Decay,
            reservoir: f32,
            error: f32,
            feed_rate: FeedRate,
        },
        Nop,
    }

    impl State {
        fn feed(&mut self, now: Instant, event: DWORD, delta: i8) -> bool {
            // Empirical feed pattern (number is `delta`)
            // slow scroll  : 1     1     1... >= 100ms interval, up to few seconds.
            // normal scroll: 1  1  1  1  1... <  100ms interval.
            // fast scroll  : 3333333333333... ~= 15ms interval, with greater `delta`
            match self {
                State::Scrolling {
                    event: prev_event,
                    scroll_direction,
                    buffer,
                    decay,
                    feed_rate,
                    ..
                } if *prev_event == event && *scroll_direction as i8 == delta.signum() => {
                    feed_rate.feed(now);
                    *buffer += delta.abs() as f32;
                    *decay = Decay::AutomaticExponential;
                    false
                }
                _ => {
                    *self = State::Scrolling {
                        event,
                        scroll_direction: delta.signum() as f32,
                        buffer: delta.abs() as f32,
                        decay: Decay::AutomaticExponential,
                        reservoir: 0.0,
                        error: 0.0,
                        feed_rate: FeedRate::new(now),
                    };
                    true
                }
            }
        }

        fn tick(&mut self) -> Option<WheelTick> {
            match *self {
                State::Scrolling {
                    event,
                    scroll_direction,
                    ref mut buffer,
                    ref mut decay,
                    ref mut reservoir,
                    ref mut error,
                    ref feed_rate,
                    ..
                } => {
                    const BUFFER_MIN_DRAIN_PER_TICK: f32 =
                        1.0 / BUFFER_MAX_DRAIN_DURATION_SECS / WHEEL_TICK_FREQ as f32;
                    let drain = if *buffer > 1.0 {
                        // Greater buffer value, faster drain.
                        *buffer * BUFFER_MIN_DRAIN_PER_TICK
                    } else {
                        // Use linear rate to eliminate long-tail
                        BUFFER_MIN_DRAIN_PER_TICK.min(*buffer)
                    };
                    *buffer -= drain;
                    *reservoir += drain;

                    // Snappier stop for fast feeds (multiplier >> 1.0),
                    // smoother decay for slow feeds (multiplier ~= 1.0).
                    let snappy_coefficient = {
                        let rate = feed_rate
                            .get(LINEAR_DECAY_DURATION_SECS)
                            // To prevent div by 0
                            .max(MIN_FEED_INTERVAL_SECS);
                        (LINEAR_DECAY_DURATION_SECS / rate).sqrt()
                    };

                    const MIN_DECAY_RATE: f32 =
                        WHEEL_TICK_INTERVAL_SECS / LINEAR_DECAY_DURATION_SECS;

                    if *buffer <= 0.1 && *decay == Decay::AutomaticExponential {
                        // The buffer is depleted. We assumes that the scrolling is stopped.
                        // The reservoir will be depleted in LINEAR_DECAY_DURATION_SECS linearly.
                        // We snapshot the linear decay rate based on the current reservoir value.
                        *decay = Decay::SnapshotLinear {
                            rate: *reservoir * MIN_DECAY_RATE * snappy_coefficient,
                        };
                    }

                    let amount = if let Decay::SnapshotLinear { rate } = decay {
                        rate.min(*reservoir)
                    } else {
                        *reservoir * MIN_DECAY_RATE * snappy_coefficient
                    };
                    *reservoir -= amount;

                    let mouse_data = {
                        let delta_f32 = scroll_direction * amount * WHEEL_DELTA as f32;
                        let mut delta = delta_f32 as i32;

                        // accumulate f32 -> i32 rounding errors.
                        *error += delta_f32 - delta as f32;
                        delta += error.div_euclid(1.0) as i32;
                        *error = error.rem_euclid(1.0);

                        delta as DWORD
                    };

                    // Cut the long-tail.
                    if *reservoir < 1.0 / WHEEL_DELTA as f32 {
                        *self = State::Nop;
                    }

                    Some(WheelTick { event, mouse_data })
                }
                State::Nop { .. } => None,
            }
        }
    }

    #[derive(Debug, PartialEq)]
    enum Decay {
        SnapshotLinear { rate: f32 },
        AutomaticExponential,
    }

    struct WheelTick {
        event: DWORD,
        mouse_data: DWORD,
    }

    #[derive(Debug)]
    struct FeedRate {
        value: Option<f32>,
        prev: Instant,
    }

    impl FeedRate {
        fn new(now: Instant) -> Self {
            Self {
                value: None,
                prev: now,
            }
        }

        fn feed(&mut self, now: Instant) {
            const MOVING_AVG_COEFF: f32 = 0.1;

            let diff = (now - self.prev).as_secs_f32();
            self.value = if let Some(value) = self.value {
                Some(value * (1.0 - MOVING_AVG_COEFF) + diff * MOVING_AVG_COEFF)
            } else {
                Some(diff)
            };

            self.prev = now;
        }

        fn get(&self, max: f32) -> f32 {
            self.value.unwrap_or(max).min(max)
        }
    }
}

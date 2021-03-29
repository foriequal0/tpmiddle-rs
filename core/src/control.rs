use std::marker::PhantomData;
use std::str::FromStr;

use thiserror::*;

use crate::mouse_hal::MouseHAL;
use crate::units::{Delta, Direction, Tick, Wheel};

#[derive(Eq, PartialEq, Copy, Clone)]
pub enum ScrollControlType {
    Classic,
    Smooth,
}

#[derive(Error, Debug)]
pub enum ScrollControlTypeParseError {
    #[error("`{0}` is an invalid type")]
    InvalidScrollControlType(String),
}

impl FromStr for ScrollControlType {
    type Err = ScrollControlTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "classic" => Ok(ScrollControlType::Classic),
            "smooth" => Ok(ScrollControlType::Smooth),
            _ => Err(ScrollControlTypeParseError::InvalidScrollControlType(
                s.to_owned(),
            )),
        }
    }
}

impl ScrollControlType {
    pub fn create_control<HAL>(&self) -> Box<dyn ScrollControl>
    where
        HAL: 'static + MouseHAL,
    {
        match self {
            ScrollControlType::Classic => Box::new(classic::ClassicController::<HAL>::new()),
            ScrollControlType::Smooth => Box::new(smooth::SmoothController::<HAL>::new()),
        }
    }
}

pub trait ScrollControl {
    fn middle_click(&self);
    fn tick(&self, tick: Wheel<Tick<i8>>);
    fn stop(&self);
}

mod classic {
    use super::*;

    pub struct ClassicController<HAL> {
        _phantom: PhantomData<HAL>,
    }

    impl<HAL> ClassicController<HAL> {
        pub fn new() -> Self {
            Self {
                _phantom: Default::default(),
            }
        }
    }

    impl<HAL> ScrollControl for ClassicController<HAL>
    where
        HAL: MouseHAL,
    {
        fn middle_click(&self) {
            HAL::send_middle_click();
        }

        fn tick(&self, tick: Wheel<Tick<i8>>) {
            HAL::send_wheel(tick.into_delta());
        }

        fn stop(&self) {}
    }
}

mod smooth {
    use super::*;

    use std::thread::{spawn, JoinHandle};
    use std::time::Instant;

    use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
    use spin_sleep::LoopHelper;

    // Empirically found min feed interval
    const MIN_FEED_INTERVAL_SECS: f32 = 0.015;
    // Treat feed intervals greater than this as a separate wheel event
    const MAX_FEED_INTERVAL_SECS: f32 = 0.3;

    const WHEEL_TICK_FREQ: u64 = 120;
    const WHEEL_TICK_INTERVAL_SECS: f32 = 1.0 / WHEEL_TICK_FREQ as f32;

    /// Time to fully drain the buffer into the reservoir.
    const BUFFER_MAX_DRAIN_DURATION_SECS: f32 = 0.05;

    pub struct SmoothController<HAL> {
        sender: Option<Sender<Event>>,
        join_handle: Option<JoinHandle<()>>,
        _phantom: PhantomData<HAL>,
    }

    impl<HAL> SmoothController<HAL>
    where
        HAL: MouseHAL,
    {
        pub fn new() -> Self {
            let ticker = Ticker::new(WHEEL_TICK_FREQ);
            let (sender, receiver) = bounded(1);
            let mut state = State::Nop;
            let join_handle = spawn(move || loop {
                crossbeam_channel::select! {
                    recv(ticker.receiver) -> _ => {
                        match state.tick() {
                            Some(delta) => HAL::send_wheel(delta),
                            None => ticker.stop(),
                        }
                    }
                    recv(receiver) -> event => {
                        match event {
                            Ok(Event::Tick(tick)) => {
                                let now = Instant::now();
                                if state.feed(now, tick) {
                                    ticker.resume();
                                }
                            }
                            Ok(Event::Stop) => {
                                state = State::Nop;
                                ticker.stop();
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                };
            });
            Self {
                sender: Some(sender),
                join_handle: Some(join_handle),
                _phantom: Default::default(),
            }
        }
    }

    impl<HAL> ScrollControl for SmoothController<HAL>
    where
        HAL: MouseHAL,
    {
        fn middle_click(&self) {
            HAL::send_middle_click();
        }

        fn tick(&self, tick: Wheel<Tick<i8>>) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(Event::Tick(tick))
                .expect("Smooth scrolling thread is dead")
        }

        fn stop(&self) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(Event::Stop)
                .expect("Smooth scrolling thread is dead")
        }
    }

    impl<HAL> Drop for SmoothController<HAL> {
        fn drop(&mut self) {
            std::mem::drop(self.sender.take());
            if let Some(join_handle) = self.join_handle.take() {
                join_handle.join().expect("Smooth scrolling thread is dead");
            }
        }
    }

    struct Ticker {
        receiver: Receiver<()>,
        sender: Option<Sender<TickerCommand>>,
        join_handle: Option<JoinHandle<()>>,
    }

    enum TickerCommand {
        Start,
        Stop,
    }

    impl Ticker {
        fn new(freq: u64) -> Self {
            let (ticker_sender, ticker_receiver) = bounded(1);
            let (command_sender, command_receiver) = bounded(1);
            let join_handle = spawn(move || 'thread: loop {
                loop {
                    match command_receiver.recv() {
                        Ok(TickerCommand::Start) => break,
                        Err(_) => break 'thread,
                        _ => {}
                    }
                }

                let mut helper = LoopHelper::builder().build_with_target_rate(freq as f32);
                loop {
                    helper.loop_start();
                    helper.loop_sleep();
                    match command_receiver.try_recv() {
                        Ok(TickerCommand::Stop) => break,
                        Err(TryRecvError::Disconnected) => break 'thread,
                        _ => {}
                    }
                    ticker_sender.send(()).expect("Ticker receiver is dead");
                }
            });

            Self {
                receiver: ticker_receiver,
                sender: Some(command_sender),
                join_handle: Some(join_handle),
            }
        }

        fn resume(&self) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(TickerCommand::Start)
                .expect("Ticker thread is dead");
        }

        fn stop(&self) {
            let sender = self.sender.as_ref().unwrap();
            sender
                .send(TickerCommand::Stop)
                .expect("Ticker thread is dead");
        }
    }

    impl Drop for Ticker {
        fn drop(&mut self) {
            std::mem::drop(self.sender.take());
            if let Some(join_handle) = self.join_handle.take() {
                join_handle.join().expect("Smooth scrolling thread is dead");
            }
        }
    }

    enum Event {
        Tick(Wheel<Tick<i8>>),
        Stop,
    }

    #[derive(Debug)]
    enum State {
        Scrolling {
            direction: Wheel<Direction>,
            buffer: f32,
            decay: Decay,
            reservoir: f32,
            error: f32,
            feed_rate: FeedRate,
        },
        Nop,
    }

    impl State {
        fn feed(&mut self, now: Instant, tick: Wheel<Tick<i8>>) -> bool {
            // Empirical feed pattern (number is `delta`)
            // slow scroll  : 1     1     1... >= 100ms interval, up to few seconds.
            // normal scroll: 1  1  1  1  1... <  100ms interval.
            // fast scroll  : 3333333333333... ~= 15ms interval, with greater `delta`
            match self {
                State::Scrolling {
                    direction: prev_dir,
                    buffer,
                    decay,
                    feed_rate,
                    ..
                } if &tick.into_direction() == prev_dir => {
                    feed_rate.feed(now, tick.value());
                    // To enable more precise wheel speed control, nudge the delta when the pressure is low,
                    // High pressure -> faster feed rate -> nudge ~ 1.0 (for a narrower range)
                    // Low pressure -> Slower feed rate -> nudge < 1.0 (for a broader range)
                    let nudge = (MIN_FEED_INTERVAL_SECS / feed_rate.interval()).sqrt();
                    let value = tick.value().raw().abs() as f32 * nudge;
                    *buffer += value;
                    *decay = Decay::AutomaticExponential;
                    false
                }
                _ => {
                    let initial_nudge = (MIN_FEED_INTERVAL_SECS / MAX_FEED_INTERVAL_SECS).sqrt();
                    *self = State::Scrolling {
                        direction: tick.into_direction(),
                        buffer: tick.value().raw().abs() as f32 * initial_nudge,
                        decay: Decay::AutomaticExponential,
                        reservoir: 0.0,
                        error: 0.0,
                        feed_rate: FeedRate::new(now),
                    };
                    true
                }
            }
        }

        fn tick(&mut self) -> Option<Wheel<Delta>> {
            match *self {
                State::Scrolling {
                    direction,
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
                    if drain > 0.0 {
                        // Capping reservoir with `feed_rate` prevents `reservoir` grows indefinitely.
                        // `reservoir` might decay slower than the `feed_rate`.
                        *reservoir = reservoir.min(feed_rate.moving_avg());
                    }

                    let feed_interval = feed_rate.interval();
                    let decay_rate = WHEEL_TICK_INTERVAL_SECS / feed_interval;

                    if *buffer == 0.0 && *decay == Decay::AutomaticExponential {
                        // The buffer is depleted. We assumes that the scrolling is stopped.
                        // To prevent long-tail of exponential decay, we'll decay `reservoir` quadratically.
                        // with linearly decreasing decay amount over `feed_interval * 2`
                        // (total sum of `amount` would be `*reservoir`)
                        // It means that the next wheel event might arrive before `reservoir` is depleted.
                        // But it'll leave a small window for jittery wheel events to continue the scroll.
                        *decay = Decay::Quadratic {
                            amount: *reservoir * decay_rate,
                            decreasing_rate: *reservoir * decay_rate
                                / (feed_interval * 2.0 * WHEEL_TICK_FREQ as f32),
                        };
                    }

                    let amount = if let Decay::Quadratic {
                        amount,
                        decreasing_rate,
                    } = decay
                    {
                        let result = amount.min(*reservoir);
                        *amount -= decreasing_rate.min(*amount);
                        result
                    } else {
                        *reservoir * decay_rate
                    };
                    *reservoir -= amount;

                    let delta = {
                        let ticks = direction.into_tick().value() * amount;
                        let (mut delta, delta_error) = ticks.into();

                        *error += delta_error;
                        delta += error.div_euclid(1.0) as i32;
                        *error = error.rem_euclid(1.0);

                        delta
                    };

                    if *reservoir == 0.0 || amount == 0.0 && *error < 1.0 {
                        *self = State::Nop;
                    }

                    Some(direction.with_value(delta))
                }
                State::Nop { .. } => None,
            }
        }
    }

    #[derive(Debug, PartialEq)]
    enum Decay {
        Quadratic { amount: f32, decreasing_rate: f32 },
        AutomaticExponential,
    }

    #[derive(Debug)]
    struct FeedRate {
        interval: Option<f32>,
        value: Option<f32>,
        prev: Instant,
    }

    impl FeedRate {
        fn new(now: Instant) -> Self {
            Self {
                interval: None,
                value: None,
                prev: now,
            }
        }

        fn feed(&mut self, now: Instant, tick: Tick<i8>) {
            const MOVING_AVG_COEFF: f32 = 0.5;

            let diff = (now - self.prev).as_secs_f32();
            self.interval = if let Some(interval) = self.interval {
                Some(interval * (1.0 - MOVING_AVG_COEFF) + diff * MOVING_AVG_COEFF)
            } else {
                Some(diff)
            };

            let tick = tick.raw().abs() as _;
            self.value = if let Some(value) = self.value {
                Some(value * (1.0 - MOVING_AVG_COEFF) + tick * MOVING_AVG_COEFF)
            } else {
                Some(tick)
            };

            self.prev = now;
        }

        fn interval(&self) -> f32 {
            self.interval
                .unwrap_or(MAX_FEED_INTERVAL_SECS)
                .min(MAX_FEED_INTERVAL_SECS)
                .max(MIN_FEED_INTERVAL_SECS)
        }

        fn moving_avg(&self) -> f32 {
            self.value.unwrap_or(1.0) / self.interval()
        }
    }
}

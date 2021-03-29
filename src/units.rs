use std::ops::{AddAssign, Mul};

/// 1 tick = 120 delta.
/// On Windows : See https://docs.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel?redirectedfrom=MSDN#parameters
/// On Linux : See https://www.kernel.org/doc/html/latest/input/event-codes.html#ev-rel
pub const WHEEL_DELTA: i32 = 120;

#[derive(Copy, Clone, Debug)]
pub struct Tick<T>(T);

impl<T> Tick<T> {
    pub fn from_raw(raw: T) -> Self {
        Tick(raw)
    }

    pub fn raw(&self) -> T
    where
        T: Copy,
    {
        self.0
    }
}

impl Mul<f32> for Tick<f32> {
    type Output = Tick<f32>;

    fn mul(self, rhs: f32) -> Self::Output {
        match self {
            Tick(raw) => Tick(raw * rhs),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Delta(i32);

impl Delta {
    pub fn from_raw(raw: i32) -> Self {
        Delta(raw)
    }

    pub fn raw(&self) -> i32 {
        self.0
    }
}

impl AddAssign<i32> for Delta {
    fn add_assign(&mut self, rhs: i32) {
        self.0 += rhs;
    }
}

impl From<Tick<i8>> for Delta {
    fn from(tick: Tick<i8>) -> Self {
        Delta(tick.raw() as i32 * WHEEL_DELTA)
    }
}

impl From<Tick<f32>> for (Delta, f32) {
    fn from(tick: Tick<f32>) -> Self {
        let delta_f32 = tick.raw() * WHEEL_DELTA as f32;
        let delta = delta_f32 as i32;
        let error = delta_f32 - delta as f32;
        (Delta(delta), error)
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub struct Direction(i8);

impl From<Tick<i8>> for Direction {
    fn from(tick: Tick<i8>) -> Self {
        Direction(tick.raw().signum())
    }
}

impl From<Direction> for Tick<f32> {
    fn from(dir: Direction) -> Self {
        Tick::from_raw(dir.0 as f32)
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum Wheel<T> {
    Vertical(T),
    Horizontal(T),
}

impl<T> Wheel<T> {
    pub fn with_value<U>(&self, value: U) -> Wheel<U> {
        match self {
            Wheel::Vertical(_) => Wheel::Vertical(value),
            Wheel::Horizontal(_) => Wheel::Vertical(value),
        }
    }

    pub fn value(&self) -> T
    where
        T: Copy,
    {
        match self {
            Wheel::Vertical(x) | Wheel::Horizontal(x) => *x,
        }
    }

    fn into<F, U>(self, map: F) -> Wheel<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Wheel::Vertical(x) => Wheel::Vertical(map(x)),
            Wheel::Horizontal(x) => Wheel::Horizontal(map(x)),
        }
    }

    pub fn into_tick<U>(self) -> Wheel<Tick<U>>
    where
        T: Into<Tick<U>>,
    {
        self.into(|x| x.into())
    }

    pub fn into_delta(self) -> Wheel<Delta>
    where
        T: Into<Delta>,
    {
        self.into(|x| x.into())
    }

    pub fn into_direction(self) -> Wheel<Direction>
    where
        T: Into<Direction>,
    {
        self.into(|x| x.into())
    }
}

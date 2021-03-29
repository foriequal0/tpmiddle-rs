use crate::units::{Delta, Wheel};

pub trait MouseHAL {
    fn send_middle_click();
    fn send_wheel(delta: Wheel<Delta>);
}

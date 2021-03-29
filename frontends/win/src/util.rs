use std::ops::{Deref, DerefMut};

#[macro_export]
macro_rules! c_try {
    ($expr:expr) => {
        unsafe {
            use ::anyhow;
            use ::winapi::shared::minwindef::{BOOL, FALSE};
            use ::winapi::um::errhandlingapi::{GetLastError, SetLastError};
            SetLastError(0);
            let result: BOOL = $expr;
            if result == FALSE {
                let last_error = GetLastError();
                if last_error != 0 {
                    Err(anyhow::anyhow!("LastError: {:x}", last_error))
                } else {
                    Ok(result)
                }
            } else {
                Ok(result)
            }
        }
    };
}

#[macro_export]
macro_rules! c_try_nonnull {
    ($expr:expr) => {
        unsafe {
            use ::anyhow;
            use ::winapi::um::errhandlingapi::{GetLastError, SetLastError};
            SetLastError(0);
            let result = $expr;
            if result as usize == 0 {
                let last_error = GetLastError();
                if last_error != 0 {
                    Err(anyhow::anyhow!("LastError: {:x}", last_error))
                } else {
                    Ok(result)
                }
            } else {
                Ok(result)
            }
        }
    };
}

#[macro_export]
macro_rules! c_try_ne_unsafe {
    ($x: expr, $expr:expr) => {{
        use ::anyhow;
        use ::winapi::um::errhandlingapi::{GetLastError, SetLastError};
        SetLastError(0);
        let result = $expr;
        if result == $x {
            let last_error = GetLastError();
            if last_error != 0 {
                Err(anyhow::anyhow!("LastError: {:x}", last_error))
            } else {
                Ok(result)
            }
        } else {
            Ok(result)
        }
    }};
}

#[macro_export]
macro_rules! c_try_ne {
    ($x: expr, $expr:expr) => {
        unsafe { c_try_ne_unsafe!($x, $expr) }
    };
}

pub struct ForceSendSync<T>(T);

unsafe impl<T> Send for ForceSendSync<T> {}
unsafe impl<T> Sync for ForceSendSync<T> {}

impl<T> ForceSendSync<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for ForceSendSync<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ForceSendSync<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

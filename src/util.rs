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
                    anyhow::bail!("LastError: {:x}", last_error);
                }
            }
            result
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
                    anyhow::bail!("LastError: {:x}", last_error);
                }
            }
            result
        }
    };
}

#[macro_export]
macro_rules! c_try_ne {
    ($x: expr, $expr:expr) => {{
        use ::anyhow;
        use ::winapi::um::errhandlingapi::{GetLastError, SetLastError};
        SetLastError(0);
        let result = $expr;
        if result == $x {
            let last_error = GetLastError();
            if last_error != 0 {
                anyhow::bail!("LastError: {:x}", last_error);
            }
        }
        result
    }};
}

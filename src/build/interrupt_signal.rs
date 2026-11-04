use std::process::ExitStatus;

pub struct InterruptSignal;

impl InterruptSignal {
    pub const NUMBER: u8 = 2;

    #[cfg(unix)]
    pub fn stopped(status: ExitStatus) -> bool {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(i32::from(Self::NUMBER))
    }

    #[cfg(not(unix))]
    pub fn stopped(_status: ExitStatus) -> bool {
        false
    }
}

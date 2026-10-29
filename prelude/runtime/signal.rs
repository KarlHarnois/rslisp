#[cfg(unix)]
pub fn restore_default_sigpipe() {
    const SIGPIPE: i32 = 13;
    const DEFAULT_HANDLER: usize = 0;
    unsafe extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
    }
    unsafe {
        signal(SIGPIPE, DEFAULT_HANDLER);
    }
}

#[cfg(not(unix))]
pub fn restore_default_sigpipe() {}

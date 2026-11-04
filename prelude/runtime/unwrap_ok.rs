#[track_caller]
pub fn unwrap_ok<T, E: Show>(result: std::result::Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("called unwrap-ok on (err {})", Shown(&error)),
    }
}

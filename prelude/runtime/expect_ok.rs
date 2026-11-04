#[track_caller]
pub fn expect_ok<T, E: Show>(result: std::result::Result<T, E>, message: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{message}: (err {})", Shown(&error)),
    }
}

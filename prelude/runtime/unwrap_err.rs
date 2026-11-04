#[track_caller]
pub fn unwrap_err<T: Show, E>(result: std::result::Result<T, E>) -> E {
    match result {
        Ok(value) => panic!("called unwrap-err on (ok {})", Shown(&value)),
        Err(error) => error,
    }
}

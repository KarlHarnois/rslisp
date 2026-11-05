pub fn check_true_with<M: std::fmt::Display>(
    location: &str,
    text: &str,
    holds: bool,
    message: impl FnOnce() -> M,
) {
    if !holds {
        fail_check("fail", location, text, &[], Some(&message().to_string()));
    }
}

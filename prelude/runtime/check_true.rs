pub fn check_true(location: &str, text: &str, holds: bool, message: Option<&str>) {
    if !holds {
        fail_check("fail", location, text, &[], message);
    }
}

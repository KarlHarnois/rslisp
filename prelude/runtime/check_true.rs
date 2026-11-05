pub fn check_true(location: &str, text: &str, holds: bool) {
    if !holds {
        fail_check("fail", location, text, &[], None);
    }
}

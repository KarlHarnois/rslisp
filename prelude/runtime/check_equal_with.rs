pub fn check_equal_with<A: Show + ?Sized, B: Show + ?Sized, M: std::fmt::Display>(
    location: &str,
    text: &str,
    left: &A,
    right: &B,
    equal: impl Fn(&A, &B) -> bool,
    message: impl FnOnce() -> M,
) {
    if !equal(left, right) {
        let sides = [Shown(left).to_string(), Shown(right).to_string()];
        fail_check("fail-equal", location, text, &sides, Some(&message().to_string()));
    }
}

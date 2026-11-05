pub fn check_equal<A: Show + ?Sized, B: Show + ?Sized>(
    location: &str,
    text: &str,
    left: &A,
    right: &B,
    equal: impl Fn(&A, &B) -> bool,
    message: Option<&str>,
) {
    if !equal(left, right) {
        let sides = [Shown(left).to_string(), Shown(right).to_string()];
        fail_check("fail-equal", location, text, &sides, message);
    }
}

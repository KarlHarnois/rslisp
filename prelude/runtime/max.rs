pub fn max<T: PartialOrd>(left: T, right: T) -> T {
    if left > right || right.partial_cmp(&right).is_none() { left } else { right }
}

pub fn min<T: PartialOrd>(left: T, right: T) -> T {
    if right < left || left.partial_cmp(&left).is_none() { right } else { left }
}

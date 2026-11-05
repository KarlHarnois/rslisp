fn fail_check(head: &str, location: &str, text: &str, sides: &[String], message: Option<&str>) {
    let parts = [location, text]
        .into_iter()
        .chain(sides.iter().map(String::as_str))
        .chain(message);
    let record = parts.fold(head.to_string(), |record, part| {
        format!("{record} {}", Shown(part))
    });
    *FAILED_RECORD.lock().unwrap() = Some(record);
    let shown_sides: String = ["left: ", "right:"]
        .iter()
        .zip(sides)
        .map(|(label, side)| format!("\n{label} {side}"))
        .collect();
    let note = message.map(|text| format!("\n{text}")).unwrap_or_default();
    panic!("assertion failed at {location}: {text}{shown_sides}{note}");
}

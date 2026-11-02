pub fn lines(reader: impl std::io::BufRead) -> impl Iterator<Item = std::io::Result<String>> {
    let mut lines = reader.lines();
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let line = lines.next()?;
        failed = line.is_err();
        Some(line)
    })
}

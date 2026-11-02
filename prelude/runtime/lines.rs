pub fn lines(_: &std::io::Stdin) -> impl Iterator<Item = std::io::Result<String>> + use<> {
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let line = read_line().transpose()?;
        failed = line.is_err();
        Some(line)
    })
}

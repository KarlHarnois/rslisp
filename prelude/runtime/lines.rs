pub fn lines(_: &std::io::Stdin) -> impl Iterator<Item = std::io::Result<String>> + use<> {
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let line = next_line().transpose()?;
        failed = line.is_err();
        Some(line)
    })
}

fn next_line() -> std::io::Result<Option<String>> {
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(Some(line))
}

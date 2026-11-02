pub fn lines(reader: impl std::io::BufRead) -> impl Iterator<Item = std::io::Result<String>> {
    let mut failed = false;
    reader.lines().take_while(move |line| {
        let before_failure = !failed;
        failed = line.is_err();
        before_failure
    })
}

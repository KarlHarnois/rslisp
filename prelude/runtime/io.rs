pub fn read_line() -> std::io::Result<Option<String>> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(error) => Err(error),
    }
}

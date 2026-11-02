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

pub trait Source {
    fn slurp(&self) -> std::io::Result<String>;
}

impl Source for str {
    fn slurp(&self) -> std::io::Result<String> {
        std::fs::read_to_string(self)
    }
}

impl Source for String {
    fn slurp(&self) -> std::io::Result<String> {
        std::fs::read_to_string(self)
    }
}

impl Source for std::io::Stdin {
    fn slurp(&self) -> std::io::Result<String> {
        std::io::read_to_string(self.lock())
    }
}

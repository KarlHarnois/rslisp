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
        std::io::Write::flush(&mut std::io::stdout())?;
        std::io::read_to_string(self.lock())
    }
}

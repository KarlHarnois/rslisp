use std::fs;
use std::io;
use std::path::Path;

use crate::failure::TaskFailure;

pub struct OutputFile<'a> {
    path: &'a Path,
}

impl<'a> OutputFile<'a> {
    pub fn new(path: &'a Path) -> Self {
        Self { path }
    }

    pub fn write_with<T>(
        &self,
        write: impl FnOnce(&Path) -> io::Result<T>,
    ) -> Result<(), TaskFailure> {
        self.create_parent()?;
        write(self.path).map_err(|error| TaskFailure::unwritable(self.path, error))?;
        Ok(())
    }

    fn create_parent(&self) -> Result<(), TaskFailure> {
        match self.path.parent() {
            Some(directory) => fs::create_dir_all(directory)
                .map_err(|error| TaskFailure::unwritable(directory, error)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_the_missing_directory_of_the_file() {
        let directory = std::env::temp_dir().join(format!("xtask-output-{}", std::process::id()));
        let path = directory.join("nested/results.tsv");
        OutputFile::new(&path)
            .write_with(|path| fs::write(path, "inventory\ttotal=1\n"))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "inventory\ttotal=1\n");
        fs::remove_dir_all(directory).unwrap();
    }
}

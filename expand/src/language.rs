pub struct Language;

impl Language {
    pub const NAME: &str = "rslisp";
    pub const EXTENSION: &str = "rslisp";
    pub const EDITION: i128 = 2026;
    pub const PROJECT_FILE_STEM: &str = "project";

    pub fn project_file_name() -> String {
        format!("{}.{}", Self::PROJECT_FILE_STEM, Self::EXTENSION)
    }
}

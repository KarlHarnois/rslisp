use std::env::consts::OS;

pub struct Platform {
    os: &'static str,
}

impl Platform {
    const LINE_COVERAGE_REFERENCE: &str = "linux";

    pub fn current() -> Self {
        Self { os: OS }
    }

    pub fn judges_line_coverage(&self) -> bool {
        self.os == Self::LINE_COVERAGE_REFERENCE
    }

    pub fn line_coverage_notice(&self) -> String {
        format!(
            "skip: {} measures different line coverage, so only {} checks the coverage floor and the README badge",
            self.os,
            Self::LINE_COVERAGE_REFERENCE
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn linux_judges_line_coverage() {
        assert!(Platform { os: "linux" }.judges_line_coverage());
    }

    #[test]
    fn macos_leaves_line_coverage_to_linux() {
        let platform = Platform { os: "macos" };
        assert!(!platform.judges_line_coverage());
        assert!(
            platform
                .line_coverage_notice()
                .contains("only linux checks")
        );
    }
}

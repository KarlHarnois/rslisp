use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::Fixture;

pub struct HeaderAgreement {
    definer: &'static str,
}

impl HeaderAgreement {
    const DOCUMENTED: &str = "noted";
    const UNDOCUMENTED: &str = "plain";

    pub fn of(definer: &'static str) -> Self {
        Self { definer }
    }

    pub fn assert_headers_agree(&self) {
        let headers = Fixture::load(&self.fixture_path()).string_definitions();
        let documented = Self::header(&headers, Self::DOCUMENTED);
        let undocumented = Self::header(&headers, Self::UNDOCUMENTED);
        assert_eq!(
            documented,
            undocumented.replacen(Self::UNDOCUMENTED, Self::DOCUMENTED, 1),
            "`{}` states its documented header in `note-docstring` unlike its `header-shape`",
            self.definer
        );
    }

    fn fixture_path(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/expand/doc_header_agreement_{}.rslisp",
            self.definer
        ))
    }

    fn header<'headers>(headers: &'headers HashMap<String, String>, name: &str) -> &'headers str {
        headers
            .get(&format!("{name}-header"))
            .expect("the fixture defines the header of each definition")
    }
}

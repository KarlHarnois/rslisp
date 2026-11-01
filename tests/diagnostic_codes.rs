use std::fs;

struct DiagnosticCoverage {
    snapshots: String,
}

impl DiagnosticCoverage {
    const CODE_DEFINITIONS: &str = include_str!("../expand/src/diagnostic/code.rs");
    const CODE_OPENING: &str = " = \"L";
    const CODE_CLOSING: char = '"';
    const SNAPSHOT_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");

    fn load() -> Self {
        let snapshots = fs::read_dir(Self::SNAPSHOT_DIRECTORY)
            .expect("the snapshot directory is readable")
            .map(|entry| fs::read_to_string(entry.expect("snapshots are listable").path()))
            .collect::<Result<String, _>>()
            .expect("snapshots are readable");
        Self { snapshots }
    }

    fn codes_without_snapshot(&self) -> Vec<String> {
        Self::defined_codes()
            .filter(|code| !self.snapshots.contains(&format!("[{code}]")))
            .collect()
    }

    fn defined_codes() -> impl Iterator<Item = String> {
        Self::CODE_DEFINITIONS
            .split(Self::CODE_OPENING)
            .skip(1)
            .filter_map(|rest| rest.split(Self::CODE_CLOSING).next())
            .map(|code| format!("L{code}"))
    }
}

#[test]
fn every_diagnostic_code_has_a_snapshot() {
    let coverage = DiagnosticCoverage::load();
    assert!(DiagnosticCoverage::defined_codes().count() > 0);
    assert_eq!(coverage.codes_without_snapshot(), Vec::<String>::new());
}

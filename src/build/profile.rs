#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    const OPTIMIZE: [&'static str; 2] = ["-C", "opt-level=3"];

    pub fn rustc_arguments(self) -> &'static [&'static str] {
        match self {
            BuildProfile::Debug => &[],
            BuildProfile::Release => &Self::OPTIMIZE,
        }
    }
}

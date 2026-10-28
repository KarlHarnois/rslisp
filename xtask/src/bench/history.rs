use super::git::Git;
use super::results::ResultSet;
use crate::failure::TaskFailure;

pub struct BenchHistory<'a> {
    git: Git<'a>,
}

impl<'a> BenchHistory<'a> {
    const SHOW_NOTE: [&'static str; 3] = ["notes", "--ref=bench", "show"];
    const NO_NOTE: &'static str = "no note found";

    pub fn new(git: Git<'a>) -> Self {
        Self { git }
    }

    pub fn note_of(&self, commit: &str) -> Result<HistoryNote, TaskFailure> {
        let output = self
            .git
            .run(&[Self::SHOW_NOTE.as_slice(), &[commit]].concat())?;
        let shown = match output.status.success() {
            true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
            false => Err(String::from_utf8_lossy(&output.stderr).into_owned()),
        };
        Self::read_note(commit, shown)
    }

    fn read_note(commit: &str, shown: Result<String, String>) -> Result<HistoryNote, TaskFailure> {
        match shown {
            Ok(text) => Ok(HistoryNote::new(commit, Some(text))),
            Err(stderr) if stderr.contains(Self::NO_NOTE) => Ok(HistoryNote::new(commit, None)),
            Err(stderr) => Err(TaskFailure::tool_failed(Git::PROGRAM, stderr.as_bytes())),
        }
    }
}

pub struct HistoryNote {
    commit: String,
    text: Option<String>,
}

impl HistoryNote {
    pub fn new(commit: &str, text: Option<String>) -> Self {
        Self {
            commit: commit.to_string(),
            text,
        }
    }

    pub fn recall(self, method_key: &str) -> Result<String, TaskFailure> {
        let text = self
            .text
            .ok_or_else(|| TaskFailure::note_missing(&self.commit))?;
        let results = ResultSet::parse(&text)
            .map_err(|malformed| TaskFailure::note_malformed(&self.commit, &malformed.line))?;
        match results.method_key() {
            Some(noted) if noted == method_key => Ok(text),
            noted => Err(TaskFailure::method_differs(&self.commit, noted, method_key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoteFixture;

    impl NoteFixture {
        const COMMIT: &'static str = "abc1234";
        const NOTED_KEY: &'static str = "1111111-2222222-3333333-4444444";
        const HEADED: &'static str = include_str!("../../fixtures/bench/headed-results.tsv");
        const HEADLESS: &'static str = include_str!("../../fixtures/bench/headless-results.tsv");

        fn note(text: &str) -> HistoryNote {
            HistoryNote::new(Self::COMMIT, Some(text.to_string()))
        }
    }

    #[test]
    fn reads_a_missing_note_from_the_error_of_git() {
        let stderr = format!("error: no note found for object {}.\n", NoteFixture::COMMIT);
        let failure = BenchHistory::read_note(NoteFixture::COMMIT, Err(stderr))
            .unwrap()
            .recall(NoteFixture::NOTED_KEY)
            .unwrap_err();
        assert!(failure.to_string().contains("has no note"));
    }

    #[test]
    fn fails_on_any_other_error_of_git() {
        let stderr = "fatal: failed to resolve 'nonsense' as a valid ref.\n".to_string();
        let failure = BenchHistory::read_note("nonsense", Err(stderr))
            .err()
            .unwrap();
        assert!(failure.to_string().contains("failed to resolve"));
    }

    #[test]
    fn recalls_a_note_measured_by_the_same_method() {
        let recalled = NoteFixture::note(NoteFixture::HEADED).recall(NoteFixture::NOTED_KEY);
        assert_eq!(recalled.unwrap(), NoteFixture::HEADED);
    }

    #[test]
    fn recalls_nothing_without_a_note() {
        let failure = HistoryNote::new(NoteFixture::COMMIT, None)
            .recall(NoteFixture::NOTED_KEY)
            .unwrap_err();
        assert!(failure.to_string().contains("has no note"));
    }

    #[test]
    fn recalls_nothing_measured_by_another_method() {
        let failure = NoteFixture::note(NoteFixture::HEADED)
            .recall("5555555-6666666-7777777-8888888")
            .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("`1111111-2222222-3333333-4444444`")
        );
    }

    #[test]
    fn recalls_nothing_from_a_note_without_a_method() {
        let failure = NoteFixture::note(NoteFixture::HEADLESS)
            .recall(NoteFixture::NOTED_KEY)
            .unwrap_err();
        assert!(failure.to_string().contains("`unknown`"));
    }

    #[test]
    fn recalls_nothing_from_a_malformed_note() {
        let failure = NoteFixture::note("inventory\texpand=1\n")
            .recall(NoteFixture::NOTED_KEY)
            .unwrap_err();
        assert!(failure.to_string().contains("inventory"));
    }
}

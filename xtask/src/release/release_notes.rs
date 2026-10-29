use serde_json::Value;

use super::version_bump::VersionBump;
use crate::failure::TaskFailure;

struct PullRequest {
    number: u64,
    title: String,
    bump: VersionBump,
}

impl PullRequest {
    fn from_json(value: &Value) -> Option<Self> {
        let labels = value["labels"].as_array()?;
        Some(Self {
            number: value["number"].as_u64()?,
            title: value["title"].as_str()?.to_string(),
            bump: VersionBump::from_labels(labels.iter().filter_map(Value::as_str)),
        })
    }

    fn bumps_past_patch(&self) -> bool {
        self.bump > VersionBump::Patch
    }

    fn note(&self) -> String {
        format!("- {} (#{})\n", self.title, self.number)
    }
}

pub struct ReleaseNotes {
    pulls: Vec<PullRequest>,
}

impl ReleaseNotes {
    pub fn parse(json: &str) -> Result<Self, TaskFailure> {
        let value: Value =
            serde_json::from_str(json).map_err(|error| TaskFailure::pulls_unparsable(&error))?;
        let pulls = value
            .as_array()
            .ok_or_else(TaskFailure::pulls_not_a_list)?
            .iter()
            .map(|pull| {
                PullRequest::from_json(pull).ok_or_else(|| TaskFailure::pull_malformed(pull))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { pulls })
    }

    pub fn render(&self) -> String {
        let (labeled, rest): (Vec<&PullRequest>, Vec<&PullRequest>) =
            self.pulls.iter().partition(|pull| pull.bumps_past_patch());
        labeled
            .into_iter()
            .chain(rest)
            .map(PullRequest::note)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseNotes;

    struct ReleaseNotesFixture;

    impl ReleaseNotesFixture {
        const PULLS: &'static str = include_str!("../../fixtures/release/pulls.json");
        const NOTES: &'static str = include_str!("../../fixtures/release/notes.md");

        fn render(json: &str) -> Result<String, String> {
            ReleaseNotes::parse(json)
                .map(|notes| notes.render())
                .map_err(|failure| failure.to_string())
        }
    }

    #[test]
    fn labeled_pull_requests_come_first_and_the_rest_keep_their_order() {
        assert_eq!(
            ReleaseNotesFixture::render(ReleaseNotesFixture::PULLS),
            Ok(ReleaseNotesFixture::NOTES.to_string())
        );
    }

    #[test]
    fn no_pull_request_writes_empty_notes() {
        assert_eq!(ReleaseNotesFixture::render("[]"), Ok(String::new()));
    }

    #[test]
    fn a_pull_request_without_a_title_is_refused() {
        assert!(ReleaseNotesFixture::render(r#"[{"number": 1, "labels": []}]"#).is_err());
    }

    #[test]
    fn text_that_is_not_json_is_refused() {
        assert!(ReleaseNotesFixture::render("not json").is_err());
    }
}

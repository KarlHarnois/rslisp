mod crate_version;
mod package_files;
mod release_notes;
mod release_version;
mod toml_text;
mod version_bump;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;

use crate::failure::TaskFailure;
use crate_version::CrateVersion;
use release_notes::ReleaseNotes;
use release_version::ReleaseVersion;
use version_bump::VersionBump;

#[derive(Args)]
pub struct NextRelease {
    #[arg(help = "The latest release tag, such as v0.1.0")]
    latest: String,
    #[arg(help = "The labels of the pull requests that the release ships")]
    labels: Vec<String>,
}

impl NextRelease {
    pub fn run(self) -> ExitCode {
        match self.next_tag() {
            Ok(tag) => {
                println!("{tag}");
                ExitCode::SUCCESS
            }
            Err(failure) => failure.report(),
        }
    }

    fn next_tag(&self) -> Result<String, TaskFailure> {
        let bump = VersionBump::from_labels(self.labels.iter().map(String::as_str));
        Ok(ReleaseVersion::parse_tag(&self.latest)?.bumped(bump).tag())
    }
}

#[derive(Args)]
pub struct SetVersion {
    #[arg(help = "The version to write into the crate, such as 0.1.1")]
    version: String,
}

impl SetVersion {
    pub fn run(self, root: &Path) -> ExitCode {
        match self.write(root) {
            Ok(()) => ExitCode::SUCCESS,
            Err(failure) => failure.report(),
        }
    }

    fn write(&self, root: &Path) -> Result<(), TaskFailure> {
        let version = ReleaseVersion::parse(&self.version)?;
        CrateVersion::new(root).set(&version)
    }
}

#[derive(Args)]
pub struct ReleaseNotesTask {
    #[arg(
        help = "A JSON list of the pull requests that the release ships, each with a number, a title and labels"
    )]
    pulls: PathBuf,
}

impl ReleaseNotesTask {
    pub fn run(self) -> ExitCode {
        match self.notes() {
            Ok(notes) => {
                print!("{notes}");
                ExitCode::SUCCESS
            }
            Err(failure) => failure.report(),
        }
    }

    fn notes(&self) -> Result<String, TaskFailure> {
        let json = fs::read_to_string(&self.pulls)
            .map_err(|error| TaskFailure::unreadable(&self.pulls, error))?;
        Ok(ReleaseNotes::parse(&json)?.render())
    }
}

#[cfg(test)]
mod tests {
    use super::NextRelease;

    struct NextReleaseFixture;

    impl NextReleaseFixture {
        const LATEST: &'static str = "v0.1.0";

        fn next(latest: &str, labels: &[&str]) -> Result<String, String> {
            let release = NextRelease {
                latest: latest.to_string(),
                labels: labels.iter().map(ToString::to_string).collect(),
            };
            release.next_tag().map_err(|failure| failure.to_string())
        }
    }

    #[test]
    fn no_label_releases_the_next_patch() {
        assert_eq!(
            NextReleaseFixture::next(NextReleaseFixture::LATEST, &[]),
            Ok("v0.1.1".to_string())
        );
    }

    #[test]
    fn the_minor_label_releases_the_next_minor() {
        assert_eq!(
            NextReleaseFixture::next(NextReleaseFixture::LATEST, &["release:minor"]),
            Ok("v0.2.0".to_string())
        );
    }

    #[test]
    fn the_major_label_releases_the_next_major() {
        assert_eq!(
            NextReleaseFixture::next(NextReleaseFixture::LATEST, &["release:major"]),
            Ok("v1.0.0".to_string())
        );
    }

    #[test]
    fn a_malformed_tag_is_refused() {
        assert!(NextReleaseFixture::next("release-1", &[]).is_err());
    }
}

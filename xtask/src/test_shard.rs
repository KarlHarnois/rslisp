use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum TestShard {
    SavedPreludeExpand,
    SavedPreludeCheck,
    SavedPreludeEmit,
    SavedPreludeRun,
    CheckAndEmit,
    Run,
    Rest,
}

impl TestShard {
    const STAGE_TESTS: [&'static str; 2] = ["--test", "stages"];
    const TEST_ARGUMENTS: &'static str = "--";
    const EXACT_NAMES: &'static str = "--exact";
    const SKIP: &'static str = "--skip";

    pub fn report_file(self) -> String {
        let value = self.to_possible_value().expect("no shard is skipped");
        format!("{}.info", value.get_name())
    }

    pub fn cargo_arguments(self) -> Vec<&'static str> {
        let mut arguments = match self {
            Self::Rest => vec![Self::TEST_ARGUMENTS, Self::EXACT_NAMES],
            _ => [
                &Self::STAGE_TESTS[..],
                &[Self::TEST_ARGUMENTS, Self::EXACT_NAMES],
            ]
            .concat(),
        };
        arguments.extend(self.test_selection());
        arguments
    }

    fn test_selection(self) -> Vec<&'static str> {
        match self {
            Self::Rest => Self::skip_every_named_test(),
            _ => self.named_tests().to_vec(),
        }
    }

    fn skip_every_named_test() -> Vec<&'static str> {
        Self::value_variants()
            .iter()
            .flat_map(|shard| shard.named_tests())
            .flat_map(|test| [Self::SKIP, test])
            .collect()
    }

    fn named_tests(self) -> &'static [&'static str] {
        match self {
            Self::SavedPreludeExpand => &["saved_prelude_expand_fixtures", "expand_fixtures"],
            Self::SavedPreludeCheck => &["saved_prelude_check_fixtures"],
            Self::SavedPreludeEmit => &[
                "saved_prelude_emit_fixtures",
                "saved_prelude_corpus_fixtures",
            ],
            Self::SavedPreludeRun => &["saved_prelude_run_fixtures"],
            Self::CheckAndEmit => &["check_fixtures", "emit_fixtures"],
            Self::Run => &["run_fixtures"],
            Self::Rest => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use super::TestShard;

    #[test]
    fn a_named_shard_runs_its_stage_tests_by_exact_name() {
        assert_eq!(
            TestShard::Run.cargo_arguments(),
            ["--test", "stages", "--", "--exact", "run_fixtures"]
        );
    }

    #[test]
    fn the_rest_skips_every_test_a_named_shard_runs() {
        let arguments = TestShard::Rest.cargo_arguments();
        for test in TestShard::value_variants()
            .iter()
            .flat_map(|shard| shard.named_tests())
        {
            let skipped = arguments.windows(2).any(|pair| pair == ["--skip", *test]);
            assert!(skipped, "the rest also runs {test}");
        }
    }

    #[test]
    fn the_rest_skips_only_exact_names() {
        assert_eq!(TestShard::Rest.cargo_arguments()[..2], ["--", "--exact"]);
    }

    #[test]
    fn the_rest_runs_every_test_target() {
        assert!(!TestShard::Rest.cargo_arguments().contains(&"--test"));
    }

    #[test]
    fn each_shard_writes_its_own_report() {
        assert_eq!(
            TestShard::SavedPreludeCheck.report_file(),
            "saved-prelude-check.info"
        );
    }
}

use super::recorded_inputs::{BoundName, LoadedModule, RecordedInputs};
use super::{Expander, Program};
use crate::core::ModulePath;
use crate::diagnostic::Diagnostics;
use crate::source::{SourceId, Span};

pub struct ModuleSurvey {
    pub program: Result<Program, Diagnostics>,
    pub names: Vec<Vec<BoundName>>,
    pub loaded: Vec<LoadedModule>,
}

impl Expander<'_> {
    pub fn survey(
        mut self,
        path: ModulePath,
        sources: &[SourceId],
        inputs: &[Span],
    ) -> ModuleSurvey {
        self.recorded_inputs = RecordedInputs::new(inputs.to_vec());
        let entry = self.load_source_module(path, sources);
        let recorded = std::mem::take(&mut self.recorded_inputs);
        ModuleSurvey {
            names: inputs
                .iter()
                .map(|input| recorded.bound_by(*input))
                .collect(),
            loaded: recorded.loaded().to_vec(),
            program: self.into_program(entry),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::modules::ModuleSources;
    use crate::source::SourceMap;

    #[test]
    fn a_survey_names_each_input_even_when_the_module_has_an_error() {
        let texts = ["(defn twice (x) (* 2 x))", "(defn twice (x) x)"];
        let mut sources = SourceMap::default();
        let ids: Vec<SourceId> = texts
            .iter()
            .map(|text| sources.add("prompt", *text))
            .collect();
        let inputs: Vec<Span> = ids
            .iter()
            .zip(texts)
            .map(|(id, text)| Span::new(*id, 0..text.len()))
            .collect();
        let survey = Expander::new(
            &mut sources,
            ModuleSources::new(PathBuf::new(), PathBuf::new()),
        )
        .survey(ModulePath::new("prompt"), &ids, &inputs);
        assert!(survey.program.is_err());
        let names: Vec<Vec<String>> = survey
            .names
            .iter()
            .map(|bound| bound.iter().map(|name| name.name.to_string()).collect())
            .collect();
        assert_eq!(names, [["twice"], ["twice"]]);
    }
}

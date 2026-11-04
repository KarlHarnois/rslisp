use crate::diagnostic::{Diagnostic, Label};
use crate::source::SourceId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleLoader {
    pub file: String,
    pub input: SourceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Entered,
    Buffered(SourceId),
}

#[derive(Clone)]
pub struct InputLocator {
    buffered: Vec<SourceId>,
    entered: String,
    loaders: Vec<ModuleLoader>,
}

impl InputLocator {
    pub fn new(buffered: Vec<SourceId>, entered: String, loaders: Vec<ModuleLoader>) -> Self {
        Self {
            buffered,
            entered,
            loaders,
        }
    }

    pub fn locate(
        &self,
        diagnostic: &Diagnostic,
        file_of: impl Fn(SourceId) -> String,
    ) -> Location {
        let is_entered = |source| file_of(source) == self.entered;
        if Self::involves(diagnostic, is_entered) {
            return Location::Entered;
        }
        let primary = diagnostic.primary().span().source();
        Self::outermost_call(diagnostic)
            .and_then(|call| self.buffered_holding(call.span().source()))
            .or_else(|| self.buffered_holding(primary))
            .or_else(|| {
                self.loader_of(&file_of(primary))
                    .filter(|input| !is_entered(*input))
            })
            .map_or(Location::Entered, Location::Buffered)
    }

    fn involves(diagnostic: &Diagnostic, is_entered: impl Fn(SourceId) -> bool) -> bool {
        is_entered(diagnostic.primary().span().source())
            || Self::calls(diagnostic).any(|call| is_entered(call.span().source()))
    }

    fn outermost_call(diagnostic: &Diagnostic) -> Option<&Label> {
        Self::calls(diagnostic).last()
    }

    fn calls(diagnostic: &Diagnostic) -> impl Iterator<Item = &Label> {
        diagnostic
            .labels()
            .filter(|label| label.marks_a_macro_call())
    }

    fn buffered_holding(&self, source: SourceId) -> Option<SourceId> {
        self.buffered.contains(&source).then_some(source)
    }

    fn loader_of(&self, file: &str) -> Option<SourceId> {
        self.loaders
            .iter()
            .find(|loader| loader.file == file)
            .map(|loader| loader.input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::DiagnosticCode;
    use crate::expand::ExpandProblems;
    use crate::source::Span;

    struct LocationFixture;

    impl LocationFixture {
        const EARLIER: usize = 1;
        const LATER: usize = 2;
        const ENTERED: usize = 3;
        const MODULE: usize = 4;
        const PRELUDE: usize = 5;
        const ENTERED_AGAIN: usize = 6;
        const MODULE_FILE: &str = "shapes.rslisp";

        fn source(index: usize) -> SourceId {
            SourceId::new(index)
        }

        fn span(index: usize) -> Span {
            Span::new(Self::source(index), 0..1)
        }

        fn file_of(source: SourceId) -> String {
            match source.index() {
                Self::MODULE => Self::MODULE_FILE.to_string(),
                Self::ENTERED_AGAIN => Self::file_of(Self::source(Self::ENTERED)),
                index => format!("<source {index}>"),
            }
        }

        fn locator(loader: usize) -> InputLocator {
            InputLocator::new(
                vec![Self::source(Self::EARLIER), Self::source(Self::LATER)],
                Self::file_of(Self::source(Self::ENTERED)),
                vec![ModuleLoader {
                    file: Self::MODULE_FILE.to_string(),
                    input: Self::source(loader),
                }],
            )
        }

        fn error_at(index: usize) -> Diagnostic {
            Diagnostic::error(
                DiagnosticCode::UNBOUND_NAME,
                "an error",
                Label::new(Self::span(index), "here"),
            )
        }

        fn called_from(error: Diagnostic, index: usize) -> Diagnostic {
            error.with_label(ExpandProblems::macro_call(Self::span(index)))
        }

        fn locate(diagnostic: &Diagnostic) -> Location {
            Self::locator(Self::LATER).locate(diagnostic, Self::file_of)
        }

        fn buffered(index: usize) -> Location {
            Location::Buffered(Self::source(index))
        }
    }

    #[test]
    fn an_error_whose_primary_label_lies_in_the_entered_input_is_its_own() {
        let error = LocationFixture::called_from(
            LocationFixture::error_at(LocationFixture::ENTERED),
            LocationFixture::EARLIER,
        );
        assert_eq!(LocationFixture::locate(&error), Location::Entered);
    }

    #[test]
    fn an_error_called_from_the_entered_input_is_its_own() {
        let error = LocationFixture::called_from(
            LocationFixture::error_at(LocationFixture::EARLIER),
            LocationFixture::ENTERED,
        );
        assert_eq!(LocationFixture::locate(&error), Location::Entered);
    }

    #[test]
    fn the_entered_input_is_known_by_its_name_in_another_source() {
        let error = LocationFixture::called_from(
            LocationFixture::error_at(LocationFixture::EARLIER),
            LocationFixture::ENTERED_AGAIN,
        );
        assert_eq!(LocationFixture::locate(&error), Location::Entered);
    }

    #[test]
    fn an_error_lies_at_the_input_of_its_outermost_macro_call() {
        let inner = LocationFixture::called_from(
            LocationFixture::error_at(LocationFixture::EARLIER),
            LocationFixture::EARLIER,
        );
        let error = LocationFixture::called_from(inner, LocationFixture::LATER);
        assert_eq!(
            LocationFixture::locate(&error),
            LocationFixture::buffered(LocationFixture::LATER)
        );
    }

    #[test]
    fn a_macro_call_outside_every_input_leaves_the_primary_label_to_decide() {
        let error = LocationFixture::called_from(
            LocationFixture::error_at(LocationFixture::EARLIER),
            LocationFixture::PRELUDE,
        );
        assert_eq!(
            LocationFixture::locate(&error),
            LocationFixture::buffered(LocationFixture::EARLIER)
        );
    }

    #[test]
    fn an_error_lies_at_the_input_of_its_primary_label() {
        let error = LocationFixture::error_at(LocationFixture::EARLIER).with_label(Label::new(
            LocationFixture::span(LocationFixture::ENTERED),
            "first",
        ));
        assert_eq!(
            LocationFixture::locate(&error),
            LocationFixture::buffered(LocationFixture::EARLIER)
        );
    }

    #[test]
    fn an_error_in_a_module_file_lies_at_the_input_that_loaded_it() {
        let error = LocationFixture::error_at(LocationFixture::MODULE);
        assert_eq!(
            LocationFixture::locate(&error),
            LocationFixture::buffered(LocationFixture::LATER)
        );
    }

    #[test]
    fn an_error_in_a_module_the_entered_input_loaded_is_its_own() {
        let error = LocationFixture::error_at(LocationFixture::MODULE);
        let location = LocationFixture::locator(LocationFixture::ENTERED)
            .locate(&error, LocationFixture::file_of);
        assert_eq!(location, Location::Entered);
    }

    #[test]
    fn an_error_located_at_no_input_is_the_entered_inputs() {
        let error = LocationFixture::error_at(LocationFixture::PRELUDE);
        assert_eq!(LocationFixture::locate(&error), Location::Entered);
    }
}

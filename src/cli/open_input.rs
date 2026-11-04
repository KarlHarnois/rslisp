use crate::diagnostic::DiagnosticCode;
use crate::reader::Reader;
use crate::source::SourceMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenPart {
    List,
    String,
}

pub struct OpenInput;

impl OpenInput {
    const CONTINUING_CODES: [DiagnosticCode; 2] = [
        DiagnosticCode::UNCLOSED_LIST,
        DiagnosticCode::UNTERMINATED_STRING,
    ];

    pub fn awaits_more(text: &str) -> bool {
        Self::open_part(text).is_some()
    }

    pub fn open_part(text: &str) -> Option<OpenPart> {
        let mut scratch = SourceMap::default();
        let source = scratch.add("", text);
        let diagnostics = Reader::new(scratch.file(source)).read().err()?;
        let ordered = diagnostics.in_source_order();
        let codes: Vec<&DiagnosticCode> = ordered
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Option<_>>()?;
        codes
            .iter()
            .all(|code| Self::CONTINUING_CODES.contains(code))
            .then(|| Self::part_left_open(&codes))
    }

    fn part_left_open(codes: &[&DiagnosticCode]) -> OpenPart {
        if codes.contains(&&DiagnosticCode::UNTERMINATED_STRING) {
            OpenPart::String
        } else {
            OpenPart::List
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenInput, OpenPart};

    #[test]
    fn an_unclosed_list_leaves_a_list_open() {
        assert_eq!(OpenInput::open_part("(defn f (x)"), Some(OpenPart::List));
    }

    #[test]
    fn an_unclosed_string_inside_a_list_leaves_the_string_open() {
        assert_eq!(OpenInput::open_part("(println \"a"), Some(OpenPart::String));
    }

    #[test]
    fn a_closed_text_or_another_reader_error_leaves_nothing_open() {
        assert_eq!(OpenInput::open_part("(+ 1 2)"), None);
        assert_eq!(OpenInput::open_part(") ("), None);
    }
}

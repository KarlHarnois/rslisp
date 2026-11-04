use rustyline::{Cmd, ConditionalEventHandler, Event, EventContext, RepeatCount};

use super::open_input::{OpenInput, OpenPart};

pub struct EnterKey;

impl EnterKey {
    const LIST_CONTINUATION: &str = "\n  ";
    const STRING_CONTINUATION: &str = "\n";

    fn command_for(line: &str, cursor: usize) -> Option<Cmd> {
        if !OpenInput::awaits_more(line) {
            return None;
        }
        let continuation = Self::continuation_after(&line[..cursor]);
        Some(Cmd::Insert(1, continuation.to_string()))
    }

    fn continuation_after(text: &str) -> &'static str {
        match OpenInput::open_part(text) {
            Some(OpenPart::String) => Self::STRING_CONTINUATION,
            Some(OpenPart::List) | None => Self::LIST_CONTINUATION,
        }
    }
}

impl ConditionalEventHandler for EnterKey {
    fn handle(
        &self,
        _event: &Event,
        _count: RepeatCount,
        _positive: bool,
        context: &EventContext,
    ) -> Option<Cmd> {
        Self::command_for(context.line(), context.pos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_inside_an_unclosed_list_inserts_a_newline_and_two_spaces() {
        assert_eq!(
            EnterKey::command_for("(defn twice (x)", 15),
            Some(Cmd::Insert(1, "\n  ".to_string()))
        );
    }

    #[test]
    fn enter_inside_an_unclosed_string_inserts_a_bare_newline() {
        assert_eq!(
            EnterKey::command_for("(println \"a", 10),
            Some(Cmd::Insert(1, "\n".to_string()))
        );
    }

    #[test]
    fn enter_after_a_closed_text_keeps_its_default() {
        assert_eq!(EnterKey::command_for("(+ 1 2)", 7), None);
    }

    #[test]
    fn enter_inside_a_closed_string_of_an_unclosed_list_inserts_a_bare_newline() {
        assert_eq!(
            EnterKey::command_for("(println \"abcd\"", 11),
            Some(Cmd::Insert(1, "\n".to_string()))
        );
    }

    #[test]
    fn enter_after_a_closed_string_of_an_unclosed_list_inserts_two_spaces() {
        assert_eq!(
            EnterKey::command_for("(println \"abcd\"", 15),
            Some(Cmd::Insert(1, "\n  ".to_string()))
        );
    }
}

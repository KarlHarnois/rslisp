use super::shapes::ShapeCatalog;
use crate::reader::{Form, FormKind, TokenKind, TokenScan};
use crate::stack::DeepRecursion;

pub struct MeaningComparison<'s> {
    shapes: &'s ShapeCatalog,
}

struct Docstring<'t> {
    text: &'t str,
}

impl<'s> MeaningComparison<'s> {
    pub fn new(shapes: &'s ShapeCatalog) -> Self {
        Self { shapes }
    }

    pub fn same_forms(&self, written: &[Form], formatted: &[Form]) -> bool {
        written.len() == formatted.len()
            && written
                .iter()
                .zip(formatted)
                .all(|(left, right)| self.same_form(left, right, false))
    }

    pub fn same_comments(&self, written: &str, formatted: &str) -> bool {
        Self::comment_texts(written) == Self::comment_texts(formatted)
    }

    fn comment_texts(text: &str) -> Vec<&str> {
        TokenScan::new(text)
            .filter(|token| token.kind == TokenKind::Comment)
            .map(|token| &text[token.range])
            .collect()
    }

    fn same_form(&self, left: &Form, right: &Form, is_docstring: bool) -> bool {
        match (left.kind(), right.kind()) {
            (FormKind::Symbol(left), FormKind::Symbol(right)) => left.name() == right.name(),
            (FormKind::Number(left), FormKind::Number(right)) => left.text() == right.text(),
            (FormKind::String(left), FormKind::String(right)) if is_docstring => {
                Docstring { text: left }.dedented() == Docstring { text: right }.dedented()
            }
            (FormKind::String(left), FormKind::String(right)) => left == right,
            (FormKind::List(left), FormKind::List(right)) => {
                DeepRecursion::guard(|| self.same_lists(left, right))
            }
            _ => false,
        }
    }

    fn same_lists(&self, left: &[Form], right: &[Form]) -> bool {
        let docstring = self.docstring_position(left);
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .enumerate()
                .all(|(index, (left, right))| self.same_form(left, right, docstring == Some(index)))
    }

    fn docstring_position(&self, items: &[Form]) -> Option<usize> {
        let head = items.first()?.as_symbol()?;
        let position = self.shapes.docstring_position(head.name())?;
        let is_string = matches!(items.get(position)?.kind(), FormKind::String(_));
        (is_string && items.len() > position + 1).then_some(position)
    }
}

impl Docstring<'_> {
    fn dedented(&self) -> String {
        let mut lines = self.text.split('\n');
        let first = lines.next().unwrap_or_default();
        let later: Vec<&str> = lines.collect();
        let shared = Self::shared_indentation(&later);
        let mut dedented = first.to_string();
        for line in later {
            dedented.push('\n');
            dedented.push_str(Self::without_indentation(line, shared));
        }
        dedented
    }

    fn shared_indentation(lines: &[&str]) -> usize {
        lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Self::indentation(line))
            .min()
            .unwrap_or(0)
    }

    fn without_indentation(line: &str, shared: usize) -> &str {
        &line[Self::indentation(line).min(shared)..]
    }

    fn indentation(line: &str) -> usize {
        line.len() - line.trim_start_matches(' ').len()
    }
}

#[cfg(test)]
mod tests {
    use super::Docstring;

    #[test]
    fn dedenting_removes_the_indentation_that_later_lines_share() {
        let docstring = Docstring {
            text: "Doubles.\n\n    Works on\n      any number.",
        };
        assert_eq!(docstring.dedented(), "Doubles.\n\nWorks on\n  any number.");
    }
}

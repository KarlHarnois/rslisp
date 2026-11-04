use super::Keyword;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StructuralKey {
    Struct,
    Field,
    Variant,
    Part,
    Bare,
}

impl StructuralKey {
    pub const ALL: [StructuralKey; 5] = [
        StructuralKey::Struct,
        StructuralKey::Field,
        StructuralKey::Variant,
        StructuralKey::Part,
        StructuralKey::Bare,
    ];

    pub fn from_keyword(symbol_name: &str) -> Option<StructuralKey> {
        let name = Keyword::field_name(symbol_name)?;
        Self::ALL.into_iter().find(|key| key.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            StructuralKey::Struct => "struct",
            StructuralKey::Field => "field",
            StructuralKey::Variant => "variant",
            StructuralKey::Part => "part",
            StructuralKey::Bare => "bare",
        }
    }

    pub fn keyword(self) -> String {
        format!("{}{}", Keyword::MARKER, self.name())
    }

    fn accepts(self, placeholder: &StructuralPiece) -> bool {
        match placeholder {
            StructuralPiece::Literal(_) => true,
            StructuralPiece::Parameter(_) | StructuralPiece::Name => {
                !matches!(self, StructuralKey::Field | StructuralKey::Part)
            }
            StructuralPiece::Parts => {
                matches!(self, StructuralKey::Struct | StructuralKey::Variant)
            }
            StructuralPiece::Field => self == StructuralKey::Field,
            StructuralPiece::Value => matches!(self, StructuralKey::Field | StructuralKey::Part),
        }
    }

    pub fn accepted_placeholders(self, parameters: usize) -> Vec<String> {
        let parameters = (0..parameters).map(StructuralPiece::Parameter);
        let named = [
            StructuralPiece::Name,
            StructuralPiece::Parts,
            StructuralPiece::Field,
            StructuralPiece::Value,
        ];
        parameters
            .chain(named)
            .filter(|piece| self.accepts(piece))
            .map(|piece| piece.placeholder())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StructuralPiece {
    Literal(String),
    Parameter(usize),
    Name,
    Parts,
    Field,
    Value,
}

impl StructuralPiece {
    const NAME: &'static str = "name";
    const PARTS: &'static str = "parts";
    const FIELD: &'static str = "field";
    const VALUE: &'static str = "value";

    fn named(text: &str) -> Option<StructuralPiece> {
        match text {
            Self::NAME => Some(StructuralPiece::Name),
            Self::PARTS => Some(StructuralPiece::Parts),
            Self::FIELD => Some(StructuralPiece::Field),
            Self::VALUE => Some(StructuralPiece::Value),
            _ => text.parse().ok().map(StructuralPiece::Parameter),
        }
    }

    pub fn placeholder(&self) -> String {
        match self {
            StructuralPiece::Literal(text) => text.clone(),
            StructuralPiece::Parameter(index) => format!("{{{index}}}"),
            StructuralPiece::Name => format!("{{{}}}", Self::NAME),
            StructuralPiece::Parts => format!("{{{}}}", Self::PARTS),
            StructuralPiece::Field => format!("{{{}}}", Self::FIELD),
            StructuralPiece::Value => format!("{{{}}}", Self::VALUE),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnfilledPlaceholder(pub String);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StructuralTemplate {
    text: String,
    pieces: Vec<StructuralPiece>,
}

impl StructuralTemplate {
    pub fn parse(
        text: &str,
        key: StructuralKey,
        parameters: usize,
    ) -> Result<StructuralTemplate, UnfilledPlaceholder> {
        let pieces = TemplateScanner::new(text).pieces()?;
        if let Some(unfilled) = pieces
            .iter()
            .find(|piece| !Self::fills(key, parameters, piece))
        {
            return Err(UnfilledPlaceholder(unfilled.placeholder()));
        }
        Ok(StructuralTemplate {
            text: text.to_string(),
            pieces,
        })
    }

    fn fills(key: StructuralKey, parameters: usize, piece: &StructuralPiece) -> bool {
        let in_range = !matches!(piece, StructuralPiece::Parameter(index) if *index >= parameters);
        in_range && key.accepts(piece)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn fill(&self, values: &dyn StructuralValues) -> String {
        self.pieces
            .iter()
            .map(|piece| match piece {
                StructuralPiece::Literal(text) => text.clone(),
                StructuralPiece::Parameter(index) => values.parameter(*index),
                StructuralPiece::Name => values.name(),
                StructuralPiece::Parts => values.parts(),
                StructuralPiece::Field => values.field(),
                StructuralPiece::Value => values.value(),
            })
            .collect()
    }
}

pub trait StructuralValues {
    fn parameter(&self, index: usize) -> String;
    fn name(&self) -> String;
    fn parts(&self) -> String;
    fn field(&self) -> String;
    fn value(&self) -> String;
}

struct TemplateScanner<'t> {
    rest: &'t str,
    literal: String,
    pieces: Vec<StructuralPiece>,
}

impl<'t> TemplateScanner<'t> {
    const OPEN: char = '{';
    const CLOSE: char = '}';
    const ESCAPED_OPEN: &'static str = "{{";
    const ESCAPED_CLOSE: &'static str = "}}";

    fn new(text: &'t str) -> Self {
        Self {
            rest: text,
            literal: String::new(),
            pieces: Vec::new(),
        }
    }

    fn pieces(mut self) -> Result<Vec<StructuralPiece>, UnfilledPlaceholder> {
        while let Some(character) = self.rest.chars().next() {
            self.scan(character)?;
        }
        self.end_literal();
        Ok(self.pieces)
    }

    fn scan(&mut self, character: char) -> Result<(), UnfilledPlaceholder> {
        if let Some(rest) = self.rest.strip_prefix(Self::ESCAPED_OPEN) {
            self.push_literal(Self::OPEN, rest);
        } else if let Some(rest) = self.rest.strip_prefix(Self::ESCAPED_CLOSE) {
            self.push_literal(Self::CLOSE, rest);
        } else if character == Self::OPEN {
            self.placeholder()?;
        } else if character == Self::CLOSE {
            return Err(UnfilledPlaceholder(Self::CLOSE.to_string()));
        } else {
            self.push_literal(character, &self.rest[character.len_utf8()..]);
        }
        Ok(())
    }

    fn push_literal(&mut self, character: char, rest: &'t str) {
        self.literal.push(character);
        self.rest = rest;
    }

    fn placeholder(&mut self) -> Result<(), UnfilledPlaceholder> {
        let inner = &self.rest[Self::OPEN.len_utf8()..];
        let Some(end) = inner.find(Self::CLOSE) else {
            return Err(UnfilledPlaceholder(self.rest.to_string()));
        };
        let written = &self.rest[..end + Self::OPEN.len_utf8() + Self::CLOSE.len_utf8()];
        let piece = StructuralPiece::named(&inner[..end])
            .ok_or_else(|| UnfilledPlaceholder(written.to_string()))?;
        self.end_literal();
        self.pieces.push(piece);
        self.rest = &inner[end + Self::CLOSE.len_utf8()..];
        Ok(())
    }

    fn end_literal(&mut self) {
        if !self.literal.is_empty() {
            self.pieces
                .push(StructuralPiece::Literal(std::mem::take(&mut self.literal)));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StructuralClause {
    templates: Vec<(StructuralKey, StructuralTemplate)>,
}

impl StructuralClause {
    pub fn new(templates: Vec<(StructuralKey, StructuralTemplate)>) -> Self {
        Self { templates }
    }

    pub fn template(&self, key: StructuralKey) -> &StructuralTemplate {
        self.templates
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, template)| template)
            .expect("a structural clause holds every key")
    }

    pub fn templates(&self) -> &[(StructuralKey, StructuralTemplate)] {
        &self.templates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NamedValues;

    impl StructuralValues for NamedValues {
        fn parameter(&self, index: usize) -> String {
            format!("parameter_{index}")
        }

        fn name(&self) -> String {
            "point".to_string()
        }

        fn parts(&self) -> String {
            ".x()".to_string()
        }

        fn field(&self) -> String {
            "x".to_string()
        }

        fn value(&self) -> String {
            "&self.x".to_string()
        }
    }

    #[test]
    fn a_template_fills_each_placeholder_and_unescapes_braces() {
        let template = StructuralTemplate::parse(
            "open({1}, \"{name}\"){parts} {{}}",
            StructuralKey::Struct,
            2,
        )
        .expect("the template parses");
        assert_eq!(
            template.fill(&NamedValues),
            "open(parameter_1, \"point\").x() {}"
        );
    }

    #[test]
    fn a_template_rejects_a_placeholder_its_key_cannot_fill() {
        assert_eq!(
            StructuralTemplate::parse(".part({field})", StructuralKey::Part, 2),
            Err(UnfilledPlaceholder("{field}".to_string()))
        );
    }

    #[test]
    fn a_template_rejects_a_parameter_past_the_method() {
        assert_eq!(
            StructuralTemplate::parse("{2}", StructuralKey::Bare, 2),
            Err(UnfilledPlaceholder("{2}".to_string()))
        );
    }

    #[test]
    fn a_template_rejects_an_unknown_placeholder() {
        assert_eq!(
            StructuralTemplate::parse("{other}", StructuralKey::Bare, 2),
            Err(UnfilledPlaceholder("{other}".to_string()))
        );
    }

    #[test]
    fn a_template_rejects_an_unclosed_placeholder() {
        assert_eq!(
            StructuralTemplate::parse("x{name", StructuralKey::Bare, 2),
            Err(UnfilledPlaceholder("{name".to_string()))
        );
    }

    #[test]
    fn a_template_rejects_a_lone_closing_brace() {
        assert_eq!(
            StructuralTemplate::parse("x}", StructuralKey::Bare, 2),
            Err(UnfilledPlaceholder("}".to_string()))
        );
    }

    #[test]
    fn each_key_lists_what_it_fills() {
        assert_eq!(
            StructuralKey::Bare.accepted_placeholders(2),
            vec!["{0}", "{1}", "{name}"]
        );
        assert_eq!(
            StructuralKey::Struct.accepted_placeholders(2),
            vec!["{0}", "{1}", "{name}", "{parts}"]
        );
        assert_eq!(
            StructuralKey::Field.accepted_placeholders(2),
            vec!["{field}", "{value}"]
        );
        assert_eq!(
            StructuralKey::Part.accepted_placeholders(2),
            vec!["{value}"]
        );
    }

    #[test]
    fn a_key_reads_from_its_keyword() {
        assert_eq!(
            StructuralKey::from_keyword(":variant"),
            Some(StructuralKey::Variant)
        );
        assert_eq!(StructuralKey::from_keyword("variant"), None);
        assert_eq!(StructuralKey::from_keyword(":other"), None);
    }
}

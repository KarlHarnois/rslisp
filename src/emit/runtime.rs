use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RuntimePart {
    Number,
    FromLiteral,
    FromFloatLiteral,
    Signed,
    Float,
    Integer,
    Sequence,
    Io,
}

impl RuntimePart {
    const ALL: [(RuntimePart, &'static str); 8] = [
        (RuntimePart::Number, "crate::rt::Number"),
        (RuntimePart::FromLiteral, "::from_literal("),
        (RuntimePart::FromFloatLiteral, "::from_float_literal("),
        (RuntimePart::Signed, "crate::rt::Signed"),
        (RuntimePart::Float, "crate::rt::Float"),
        (RuntimePart::Integer, "crate::rt::Integer"),
        (RuntimePart::Sequence, "crate::rt::Sequence"),
        (RuntimePart::Io, "crate::rt::read_line"),
    ];

    fn required(self) -> Option<RuntimePart> {
        match self {
            RuntimePart::FromLiteral
            | RuntimePart::FromFloatLiteral
            | RuntimePart::Signed
            | RuntimePart::Integer => Some(RuntimePart::Number),
            RuntimePart::Float => Some(RuntimePart::Signed),
            RuntimePart::Number | RuntimePart::Sequence | RuntimePart::Io => None,
        }
    }
}

struct Conversion {
    part: RuntimePart,
    name: &'static str,
    input: &'static str,
}

pub struct RuntimeModule {
    parts: BTreeSet<RuntimePart>,
}

impl RuntimeModule {
    const SIGNED: &'static str = include_str!("../../prelude/runtime/signed.rs");
    const FLOAT: &'static str = include_str!("../../prelude/runtime/float.rs");
    const INTEGER: &'static str = include_str!("../../prelude/runtime/integer.rs");
    const SEQUENCE: &'static str = include_str!("../../prelude/runtime/sequence.rs");
    const IO: &'static str = include_str!("../../prelude/runtime/io.rs");
    const FIXED_PARTS: [(RuntimePart, &'static str); 5] = [
        (RuntimePart::Signed, Self::SIGNED),
        (RuntimePart::Float, Self::FLOAT),
        (RuntimePart::Integer, Self::INTEGER),
        (RuntimePart::Sequence, Self::SEQUENCE),
        (RuntimePart::Io, Self::IO),
    ];
    const CONVERSIONS: &'static [Conversion] = &[
        Conversion {
            part: RuntimePart::FromLiteral,
            name: "from_literal",
            input: "i128",
        },
        Conversion {
            part: RuntimePart::FromFloatLiteral,
            name: "from_float_literal",
            input: "f64",
        },
    ];
    const NUMBER_TRAIT: &'static str = "pub trait Number:\n    Copy + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Rem<Output = Self>\n";
    const NUMBER_IMPLEMENTATIONS: &'static str = "macro_rules! numbers {\n    ($($number:ty),*) => {\n        $(\n            impl Number for $number ";
    const NUMBER_IMPLEMENTATIONS_END: &'static str = "\n        )*\n    };\n}\n\nnumbers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);";
    const IMPLEMENTATION_INDENT: &'static str = "            ";

    pub fn of(code: &str) -> Option<RuntimeModule> {
        let mut parts: BTreeSet<RuntimePart> = RuntimePart::ALL
            .into_iter()
            .filter(|(_, marker)| code.contains(marker))
            .map(|(part, _)| part)
            .collect();
        let mut unchecked: Vec<RuntimePart> = parts.iter().copied().collect();
        while let Some(part) = unchecked.pop() {
            if let Some(required) = part.required()
                && parts.insert(required)
            {
                unchecked.push(required);
            }
        }
        (!parts.is_empty()).then_some(RuntimeModule { parts })
    }

    pub fn source(&self) -> String {
        let mut sections = Vec::new();
        if self.parts.contains(&RuntimePart::Number) {
            sections.push(self.arithmetic_imports());
            sections.push(self.number_trait());
            sections.push(self.number_implementations());
        }
        for (part, text) in Self::FIXED_PARTS {
            if self.parts.contains(&part) {
                sections.push(text.trim_end().to_string());
            }
        }
        sections.join("\n\n")
    }

    fn arithmetic_imports(&self) -> String {
        let negation = if self.parts.contains(&RuntimePart::Signed) {
            "Neg, "
        } else {
            ""
        };
        format!("use std::ops::{{Add, Div, Mul, {negation}Rem, Sub}};")
    }

    fn number_trait(&self) -> String {
        let declarations: Vec<String> = self
            .conversions()
            .map(|conversion| {
                format!(
                    "    fn {}(value: {}) -> Self;",
                    conversion.name, conversion.input
                )
            })
            .collect();
        format!("{}{}", Self::NUMBER_TRAIT, Self::body(&declarations, ""))
    }

    fn number_implementations(&self) -> String {
        let indent = Self::IMPLEMENTATION_INDENT;
        let definitions: Vec<String> = self
            .conversions()
            .map(|conversion| {
                format!(
                    "{indent}    fn {}(value: {}) -> Self {{\n{indent}        value as Self\n{indent}    }}",
                    conversion.name, conversion.input
                )
            })
            .collect();
        format!(
            "{}{}{}",
            Self::NUMBER_IMPLEMENTATIONS,
            Self::body(&definitions, indent),
            Self::NUMBER_IMPLEMENTATIONS_END
        )
    }

    fn conversions(&self) -> impl Iterator<Item = &'static Conversion> {
        Self::CONVERSIONS
            .iter()
            .filter(|conversion| self.parts.contains(&conversion.part))
    }

    fn body(members: &[String], indent: &str) -> String {
        if members.is_empty() {
            return "{}".to_string();
        }
        format!("{{\n{}\n{indent}}}", members.join("\n\n"))
    }
}

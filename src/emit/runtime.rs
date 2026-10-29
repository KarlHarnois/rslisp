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
    MapSequence,
    Io,
    Signal,
}

impl RuntimePart {
    const STDOUT_MACROS: [&'static str; 2] = ["print!(", "println!("];
    const STDOUT_HANDLE: &'static str = "std::io::stdout(";
    const ALL: [(RuntimePart, &'static str); 10] = [
        (RuntimePart::Number, "crate::rt::Number"),
        (RuntimePart::FromLiteral, "::from_literal("),
        (RuntimePart::FromFloatLiteral, "::from_float_literal("),
        (RuntimePart::Signed, "crate::rt::Signed"),
        (RuntimePart::Float, "crate::rt::Float"),
        (RuntimePart::Integer, "crate::rt::Integer"),
        (RuntimePart::Sequence, "crate::rt::Sequence"),
        (RuntimePart::MapSequence, "std::collections::BTreeMap"),
        (RuntimePart::Io, "crate::rt::read_line"),
        (RuntimePart::Signal, Self::STDOUT_HANDLE),
    ];

    fn present(self, code: &str) -> bool {
        let marker = Self::ALL
            .iter()
            .find(|(part, _)| *part == self)
            .map(|(_, marker)| *marker)
            .expect("every runtime part has a marker");
        match self {
            Self::MapSequence => {
                (code.contains("std::collections::BTreeMap")
                    || code.contains("std::collections::HashMap"))
                    && code.contains("crate::rt::Sequence")
            }
            Self::Signal => code.contains(marker) || Self::calls_stdout_macro(code),
            _ => code.contains(marker),
        }
    }

    fn calls_stdout_macro(code: &str) -> bool {
        Self::STDOUT_MACROS.iter().any(|call| {
            code.match_indices(call)
                .any(|(start, _)| !Self::continues_identifier(&code[..start]))
        })
    }

    fn continues_identifier(preceding: &str) -> bool {
        preceding
            .chars()
            .next_back()
            .is_some_and(|character| character.is_alphanumeric() || character == '_')
    }

    fn required(self) -> Option<RuntimePart> {
        match self {
            RuntimePart::FromLiteral
            | RuntimePart::FromFloatLiteral
            | RuntimePart::Signed
            | RuntimePart::Integer => Some(RuntimePart::Number),
            RuntimePart::Float => Some(RuntimePart::Signed),
            RuntimePart::Number | RuntimePart::Sequence | RuntimePart::Io | RuntimePart::Signal => {
                None
            }
            RuntimePart::MapSequence => Some(RuntimePart::Sequence),
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
    const MAP_SEQUENCE: &'static str = include_str!("../../prelude/runtime/map_sequence.rs");
    const IO: &'static str = include_str!("../../prelude/runtime/io.rs");
    const SIGNAL: &'static str = include_str!("../../prelude/runtime/signal.rs");
    const START_UP_CALL: &'static str = "crate::rt::restore_default_sigpipe();";
    const FIXED_PARTS: [(RuntimePart, &'static str); 7] = [
        (RuntimePart::Signed, Self::SIGNED),
        (RuntimePart::Float, Self::FLOAT),
        (RuntimePart::Integer, Self::INTEGER),
        (RuntimePart::Sequence, Self::SEQUENCE),
        (RuntimePart::MapSequence, Self::MAP_SEQUENCE),
        (RuntimePart::Io, Self::IO),
        (RuntimePart::Signal, Self::SIGNAL),
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
            .map(|(part, _)| part)
            .filter(|part| part.present(code))
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

    pub fn start_up_call(&self) -> Option<&'static str> {
        self.parts
            .contains(&RuntimePart::Signal)
            .then_some(Self::START_UP_CALL)
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

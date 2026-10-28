#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResultHeader {
    fields: Vec<HeaderField>,
}

#[derive(Debug, PartialEq, Eq)]
struct HeaderField {
    name: String,
    value: String,
}

impl ResultHeader {
    const LINE_PREFIX: &'static str = "# ";
    const NAME_SEPARATOR: char = ' ';
    const METHOD: &'static str = "method";
    const RUSTC: &'static str = "rustc";
    const VALGRIND: &'static str = "valgrind";

    pub fn new(method_key: String, rustc_version: String, valgrind_version: String) -> Self {
        Self {
            fields: vec![
                HeaderField::new(Self::METHOD, method_key),
                HeaderField::new(Self::RUSTC, rustc_version),
                HeaderField::new(Self::VALGRIND, valgrind_version),
            ],
        }
    }

    pub fn read(text: &str) -> Self {
        let fields = text.lines().map_while(Self::parse_field).collect();
        Self { fields }
    }

    pub fn method_key(&self) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.name == Self::METHOD)
            .map(|field| field.value.as_str())
    }

    pub fn line_count(&self) -> usize {
        self.fields.len()
    }

    pub fn serialize(&self) -> String {
        self.fields
            .iter()
            .map(|field| {
                format!(
                    "{}{}{}{}\n",
                    Self::LINE_PREFIX,
                    field.name,
                    Self::NAME_SEPARATOR,
                    field.value
                )
            })
            .collect()
    }

    fn parse_field(line: &str) -> Option<HeaderField> {
        let field = line.strip_prefix(Self::LINE_PREFIX)?;
        let (name, value) = field
            .split_once(Self::NAME_SEPARATOR)
            .unwrap_or((field, ""));
        Some(HeaderField::new(name, value.to_string()))
    }
}

impl HeaderField {
    fn new(name: &str, value: String) -> Self {
        Self {
            name: name.to_string(),
            value,
        }
    }
}

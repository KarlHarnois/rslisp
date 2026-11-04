use std::fmt;
use std::io::{self, Write};

use serde_json::Value;

pub struct JsonReplies {
    id: Option<String>,
}

pub struct JsonObject {
    fields: Vec<(&'static str, String)>,
}

impl JsonReplies {
    const ID: &'static str = "id";

    pub fn new(id: Option<String>) -> Self {
        Self { id }
    }

    pub fn send(&self, reply: JsonObject) -> io::Result<()> {
        let mut standard_output = io::stdout().lock();
        writeln!(standard_output, "{}", self.line(reply))?;
        standard_output.flush()
    }

    fn line(&self, reply: JsonObject) -> String {
        match &self.id {
            Some(id) => JsonObject::new(Self::ID, id.as_str())
                .with_fields_of(reply)
                .to_string(),
            None => reply.to_string(),
        }
    }
}

impl JsonObject {
    pub fn new(key: &'static str, value: impl Into<Value>) -> Self {
        Self { fields: Vec::new() }.with(key, value)
    }

    pub fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.fields.push((key, value.into().to_string()));
        self
    }

    pub fn listing(key: &'static str, objects: Vec<JsonObject>) -> Self {
        let rendered: Vec<String> = objects.iter().map(JsonObject::to_string).collect();
        Self {
            fields: vec![(key, format!("[{}]", rendered.join(",")))],
        }
    }

    fn with_fields_of(mut self, other: JsonObject) -> Self {
        self.fields.extend(other.fields);
        self
    }
}

impl fmt::Display for JsonObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fields: Vec<String> = self
            .fields
            .iter()
            .map(|(key, value)| format!("{}:{value}", Value::from(*key)))
            .collect();
        write!(formatter, "{{{}}}", fields.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::{JsonObject, JsonReplies};

    #[test]
    fn a_reply_without_an_id_holds_only_its_fields_in_order() {
        let reply = JsonObject::new("value", "6").with("form", 1);
        assert_eq!(
            JsonReplies::new(None).line(reply),
            r#"{"value":"6","form":1}"#
        );
    }

    #[test]
    fn a_reply_with_an_id_starts_with_the_id() {
        let reply = JsonObject::new("out", "a \"quoted\"\n");
        assert_eq!(
            JsonReplies::new(Some("7".to_string())).line(reply),
            r#"{"id":"7","out":"a \"quoted\"\n"}"#
        );
    }

    #[test]
    fn an_object_holds_a_list_of_objects_in_order() {
        let inner = JsonObject::new("severity", "note").with("line", 1);
        let reply = JsonObject::listing("diagnostics", vec![inner]);
        assert_eq!(
            reply.to_string(),
            r#"{"diagnostics":[{"severity":"note","line":1}]}"#
        );
    }
}

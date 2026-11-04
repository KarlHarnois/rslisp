use std::rc::Rc;

use crate::expand::BoundName;
use crate::source::{SourceId, Span};

#[derive(Clone)]
pub struct BufferedInput {
    name: String,
    span: Span,
    names: Vec<BoundName>,
}

#[derive(Clone, Default)]
pub struct DefinitionBuffer {
    inputs: Vec<BufferedInput>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Replacement {
    replaced: Vec<usize>,
}

pub struct DroppedNames {
    pub input: BufferedInput,
    pub dropped: Vec<BoundName>,
    pub replacing: Rc<str>,
}

impl BufferedInput {
    pub fn new(name: String, span: Span, names: Vec<BoundName>) -> Self {
        Self { name, span, names }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn names(&self) -> &[BoundName] {
        &self.names
    }

    pub fn source(&self) -> SourceId {
        self.span.source()
    }

    pub fn binds(&self, name: &BoundName) -> bool {
        self.names.iter().any(|bound| bound.names_the_same(name))
    }
}

impl DefinitionBuffer {
    pub fn inputs(&self) -> &[BufferedInput] {
        &self.inputs
    }

    pub fn sources(&self) -> Vec<SourceId> {
        self.sources_kept_by(&Replacement::default())
    }

    pub fn spans(&self) -> Vec<Span> {
        self.inputs.iter().map(|input| input.span).collect()
    }

    pub fn bound_names(&self) -> impl Iterator<Item = &BoundName> {
        self.inputs.iter().flat_map(|input| &input.names)
    }

    pub fn sources_kept_by(&self, replacement: &Replacement) -> Vec<SourceId> {
        self.inputs
            .iter()
            .enumerate()
            .filter(|(position, _)| !replacement.replaces(*position))
            .map(|(_, input)| input.source())
            .collect()
    }

    pub fn widened(&self, names: &[BoundName], replacement: &Replacement) -> Option<Replacement> {
        let sharing: Vec<usize> = self
            .inputs
            .iter()
            .enumerate()
            .filter(|(position, input)| {
                !replacement.replaces(*position) && Self::shares_a_name(&input.names, names)
            })
            .map(|(position, _)| position)
            .collect();
        if sharing.is_empty() {
            return None;
        }
        let mut replaced = replacement.replaced.clone();
        replaced.extend(sharing);
        Some(Replacement { replaced })
    }

    pub fn dropped_by(
        &self,
        replacement: &Replacement,
        entered: &BufferedInput,
    ) -> Vec<DroppedNames> {
        replacement
            .replaced
            .iter()
            .filter_map(|position| Self::drop_of(&self.inputs[*position], entered))
            .collect()
    }

    pub fn replaced(&self, replacement: &Replacement, input: BufferedInput) -> Self {
        let place = replacement.first();
        let mut entered = Some(input);
        let mut inputs = Vec::with_capacity(self.inputs.len() + 1);
        for (position, earlier) in self.inputs.iter().enumerate() {
            if Some(position) == place {
                inputs.extend(entered.take());
            }
            if !replacement.replaces(position) {
                inputs.push(earlier.clone());
            }
        }
        inputs.extend(entered);
        Self { inputs }
    }

    pub fn renamed(&self, names: Vec<Vec<BoundName>>) -> Self {
        let inputs = self
            .inputs
            .iter()
            .zip(names)
            .map(|(input, names)| BufferedInput {
                names,
                ..input.clone()
            })
            .collect();
        Self { inputs }
    }

    pub fn without(&self, set_aside: &[SourceId]) -> Self {
        let inputs = self
            .inputs
            .iter()
            .filter(|input| !set_aside.contains(&input.source()))
            .cloned()
            .collect();
        Self { inputs }
    }

    pub fn input(&self, source: SourceId) -> Option<&BufferedInput> {
        self.inputs.iter().find(|input| input.source() == source)
    }

    fn drop_of(earlier: &BufferedInput, entered: &BufferedInput) -> Option<DroppedNames> {
        let dropped: Vec<BoundName> = earlier
            .names
            .iter()
            .filter(|name| !entered.binds(name))
            .cloned()
            .collect();
        let replacing = entered.names.iter().find(|bound| earlier.binds(bound))?;
        (!dropped.is_empty()).then(|| DroppedNames {
            input: earlier.clone(),
            dropped,
            replacing: Rc::clone(&replacing.name),
        })
    }

    fn shares_a_name(earlier: &[BoundName], names: &[BoundName]) -> bool {
        earlier
            .iter()
            .any(|name| names.iter().any(|bound| bound.names_the_same(name)))
    }
}

impl Replacement {
    fn replaces(&self, position: usize) -> bool {
        self.replaced.contains(&position)
    }

    fn first(&self) -> Option<usize> {
        self.replaced.iter().min().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Namespace;

    struct BufferFixture;

    impl BufferFixture {
        fn input(source: usize, names: &[&str]) -> BufferedInput {
            let source = SourceId::new(source);
            let names = names
                .iter()
                .map(|name| BoundName {
                    name: (*name).into(),
                    namespace: Namespace::Value,
                    span: Span::new(source, 0..0),
                })
                .collect();
            BufferedInput::new(
                format!("<input {}>", source.index()),
                Span::new(source, 0..1),
                names,
            )
        }

        fn buffer(inputs: &[(usize, &[&str])]) -> DefinitionBuffer {
            DefinitionBuffer {
                inputs: inputs
                    .iter()
                    .map(|(source, names)| Self::input(*source, names))
                    .collect(),
            }
        }

        fn sources(buffer: &DefinitionBuffer) -> Vec<usize> {
            buffer.sources().into_iter().map(SourceId::index).collect()
        }
    }

    #[test]
    fn a_replacing_input_takes_the_place_of_the_first_input_it_replaces() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"]), (2, &["c"])]);
        let entered = BufferFixture::input(3, &["b"]);
        let replacement = buffer
            .widened(&entered.names, &Replacement::default())
            .expect("`b` is defined earlier");
        let replaced = buffer.replaced(&replacement, entered);
        assert_eq!(BufferFixture::sources(&replaced), [0, 3, 2]);
    }

    #[test]
    fn an_input_that_replaces_two_inputs_takes_the_place_of_the_earlier() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"]), (2, &["c"])]);
        let entered = BufferFixture::input(3, &["a", "c"]);
        let first = buffer
            .widened(&entered.names[..1], &Replacement::default())
            .expect("`a` is defined earlier");
        let both = buffer
            .widened(&entered.names, &first)
            .expect("`c` is defined earlier");
        let replaced = buffer.replaced(&both, entered);
        assert_eq!(BufferFixture::sources(&replaced), [3, 1]);
    }

    #[test]
    fn a_new_input_joins_the_end() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"])]);
        let replaced = buffer.replaced(&Replacement::default(), BufferFixture::input(2, &["c"]));
        assert_eq!(BufferFixture::sources(&replaced), [0, 1, 2]);
    }

    #[test]
    fn a_replacement_widens_only_by_inputs_it_does_not_hold() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"])]);
        let names = BufferFixture::input(2, &["a", "b"]).names;
        let first = buffer
            .widened(&names[..1], &Replacement::default())
            .expect("`a` is defined earlier");
        let second = buffer
            .widened(&names, &first)
            .expect("`b` is defined earlier");
        assert_eq!(buffer.widened(&names, &second), None);
        assert_eq!(buffer.sources_kept_by(&second), []);
    }

    #[test]
    fn a_replacement_that_keeps_every_name_drops_nothing() {
        let buffer = BufferFixture::buffer(&[(0, &["a", "b"])]);
        let entered = BufferFixture::input(1, &["b", "a"]);
        let replacement = buffer
            .widened(&entered.names, &Replacement::default())
            .expect("`a` is defined earlier");
        assert!(buffer.dropped_by(&replacement, &entered).is_empty());
    }

    #[test]
    fn a_replacement_that_loses_a_name_drops_it() {
        let buffer = BufferFixture::buffer(&[(0, &["a", "b"])]);
        let entered = BufferFixture::input(1, &["b"]);
        let replacement = buffer
            .widened(&entered.names, &Replacement::default())
            .expect("`b` is defined earlier");
        let dropped = buffer.dropped_by(&replacement, &entered);
        let names: Vec<&str> = dropped[0].dropped.iter().map(|name| &*name.name).collect();
        assert_eq!(names, ["a"]);
        assert_eq!(&*dropped[0].replacing, "b");
    }

    #[test]
    fn a_renamed_buffer_takes_each_inputs_new_names() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"])]);
        let renamed = buffer.renamed(vec![Vec::new(), BufferFixture::input(1, &["c"]).names]);
        let names: Vec<&str> = renamed.bound_names().map(|name| &*name.name).collect();
        assert_eq!(names, ["c"]);
    }

    #[test]
    fn a_buffer_without_an_input_keeps_the_others_in_order() {
        let buffer = BufferFixture::buffer(&[(0, &["a"]), (1, &["b"]), (2, &["c"])]);
        let kept = buffer.without(&[SourceId::new(1)]);
        assert_eq!(BufferFixture::sources(&kept), [0, 2]);
        assert!(kept.input(SourceId::new(1)).is_none());
        assert!(kept.input(SourceId::new(2)).is_some());
    }
}

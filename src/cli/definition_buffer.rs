use crate::diagnostic::Diagnostic;
use crate::expand::{BoundName, ExpandProblems};
use crate::source::SourceId;

#[derive(Clone)]
pub struct BufferedInput {
    pub source: SourceId,
    pub names: Vec<BoundName>,
}

#[derive(Clone, Default)]
pub struct DefinitionBuffer {
    inputs: Vec<BufferedInput>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Replacement {
    replaced: Vec<usize>,
}

impl DefinitionBuffer {
    pub fn sources(&self) -> Vec<SourceId> {
        self.sources_kept_by(&Replacement::default())
    }

    pub fn sources_kept_by(&self, replacement: &Replacement) -> Vec<SourceId> {
        self.inputs
            .iter()
            .enumerate()
            .filter(|(position, _)| !replacement.replaces(*position))
            .map(|(_, input)| input.source)
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

    pub fn dropped_definition(
        &self,
        input: &BufferedInput,
        replacement: &Replacement,
    ) -> Option<Diagnostic> {
        replacement
            .replaced
            .iter()
            .find_map(|position| Self::drop_of(&self.inputs[*position], input))
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

    fn drop_of(earlier: &BufferedInput, input: &BufferedInput) -> Option<Diagnostic> {
        let dropped = earlier
            .names
            .iter()
            .find(|name| !input.names.iter().any(|bound| bound.names_the_same(name)))?;
        let (repeated, previous) = input.names.iter().find_map(|bound| {
            let previous = earlier
                .names
                .iter()
                .find(|name| name.names_the_same(bound))?;
            Some((bound, previous))
        })?;
        Some(ExpandProblems::dropped_by_replacement(
            &repeated.name,
            repeated.span,
            previous.span,
            &dropped.name,
        ))
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
    use crate::source::Span;

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
            BufferedInput { source, names }
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
        assert!(buffer.dropped_definition(&entered, &replacement).is_none());
    }

    #[test]
    fn a_replacement_that_loses_a_name_drops_it() {
        let buffer = BufferFixture::buffer(&[(0, &["a", "b"])]);
        let entered = BufferFixture::input(1, &["b"]);
        let replacement = buffer
            .widened(&entered.names, &Replacement::default())
            .expect("`b` is defined earlier");
        let dropped = buffer
            .dropped_definition(&entered, &replacement)
            .expect("`a` would be dropped");
        assert_eq!(
            dropped.notes(),
            ["replacing the earlier input would drop `a`"]
        );
    }
}

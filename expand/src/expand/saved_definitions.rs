use std::rc::Rc;

use rustc_hash::FxHashSet;

use crate::core::{
    BindingArena, BindingId, CoreModule, Expr, Item, ItemKind, ModuleId, References,
};

#[derive(Default, serde::Serialize)]
pub struct SavedTables {
    entries: Vec<u8>,
    references: Vec<u8>,
    items: Vec<u8>,
}

#[derive(Clone, Copy, Default)]
pub struct SavedDefinitions {
    entries: &'static [u8],
    references: &'static [u8],
    items: &'static [u8],
}

impl<'de: 'static> serde::Deserialize<'de> for SavedDefinitions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (entries, references, items) = serde::Deserialize::deserialize(deserializer)?;
        Ok(Self {
            entries,
            references,
            items,
        })
    }
}

#[derive(Clone, Copy)]
struct SavedItem {
    binding: u32,
    module: u32,
    position: u32,
    item_start: u32,
    item_end: u32,
    references_start: u32,
    references_end: u32,
}

impl SavedItem {
    const WORD: usize = size_of::<u32>();
    const WORDS: usize = 7;
    const SIZE: usize = Self::WORDS * Self::WORD;

    fn read(bytes: &[u8]) -> Self {
        let word = |index: usize| SavedWords::read(&bytes[index * Self::WORD..]);
        Self {
            binding: word(0),
            module: word(1),
            position: word(2),
            item_start: word(3),
            item_end: word(4),
            references_start: word(5),
            references_end: word(6),
        }
    }

    fn write(self, bytes: &mut Vec<u8>) {
        for word in [
            self.binding,
            self.module,
            self.position,
            self.item_start,
            self.item_end,
            self.references_start,
            self.references_end,
        ] {
            bytes.extend(word.to_le_bytes());
        }
    }
}

struct SavedWords;

impl SavedWords {
    fn read(bytes: &[u8]) -> u32 {
        let (word, _) = bytes.split_at(SavedItem::WORD);
        u32::from_le_bytes(word.try_into().expect("a saved word has four bytes"))
    }

    fn index(value: usize) -> u32 {
        u32::try_from(value).expect("the saved prelude stays under 4 GiB")
    }
}

impl SavedDefinitions {
    fn count(&self) -> usize {
        self.entries.len() / SavedItem::SIZE
    }

    fn entry(&self, index: usize) -> SavedItem {
        SavedItem::read(&self.entries[index * SavedItem::SIZE..])
    }

    fn entries(&self) -> impl Iterator<Item = SavedItem> + '_ {
        (0..self.count()).map(|index| self.entry(index))
    }

    fn first_entry_at_or_after(&self, binding: u32) -> usize {
        let (mut low, mut high) = (0, self.count());
        while low < high {
            let middle = low.midpoint(high);
            if self.entry(middle).binding < binding {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    fn entries_of(&self, binding: BindingId) -> impl Iterator<Item = SavedItem> + '_ {
        let wanted = binding.index();
        (self.first_entry_at_or_after(wanted)..self.count())
            .map(|index| self.entry(index))
            .take_while(move |entry| entry.binding == wanted)
    }

    fn item(&self, entry: SavedItem) -> Item {
        let encoded = &self.items[entry.item_start as usize..entry.item_end as usize];
        bincode::deserialize(encoded).expect("a saved prelude item decodes")
    }

    fn references(&self, entry: SavedItem) -> impl Iterator<Item = BindingId> + '_ {
        self.references[entry.references_start as usize..entry.references_end as usize]
            .chunks_exact(SavedItem::WORD)
            .map(|word| BindingId::from_index(SavedWords::read(word)))
    }

    pub fn value_of(&self, binding: BindingId) -> Option<Rc<Expr>> {
        self.entries_of(binding)
            .find_map(|entry| match self.item(entry).kind {
                ItemKind::Def(_, value) => Some(value),
                _ => None,
            })
    }
}

#[derive(Default)]
pub struct PendingDefinitions {
    saved: SavedDefinitions,
    restored: FxHashSet<BindingId>,
}

impl PendingDefinitions {
    pub fn new(saved: SavedDefinitions) -> Self {
        Self {
            saved,
            restored: FxHashSet::default(),
        }
    }

    pub fn saved_bindings(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.saved
            .entries()
            .map(|entry| BindingId::from_index(entry.binding))
    }

    pub fn undecoded(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.saved
            .entries()
            .map(|entry| BindingId::from_index(entry.binding))
            .filter(|binding| !self.restored.contains(binding))
    }

    pub fn restore(&mut self, modules: &mut [CoreModule], roots: Vec<BindingId>) {
        let mut found = self.reached_entries(roots);
        found.sort_by_key(|entry| (entry.module, entry.position));
        for entry in found {
            let module = ModuleId::new(entry.module as usize);
            let holder = modules.iter_mut().find(|candidate| candidate.id == module);
            let holder = holder.expect("a saved item names a loaded module");
            holder.items.push(self.saved.item(entry));
        }
    }

    fn reached_entries(&mut self, roots: Vec<BindingId>) -> Vec<SavedItem> {
        let mut pending = roots;
        let mut found = Vec::new();
        while let Some(binding) = pending.pop() {
            if self.restored.insert(binding) {
                for entry in self.saved.entries_of(binding) {
                    pending.extend(self.saved.references(entry));
                    found.push(entry);
                }
            }
        }
        found
    }
}

struct EncodedItem {
    entry: SavedItem,
    item: Vec<u8>,
    references: Vec<BindingId>,
}

pub struct DefinitionSaver<'a> {
    bindings: &'a BindingArena,
    kept: FxHashSet<BindingId>,
    encoded: Vec<EncodedItem>,
}

impl<'a> DefinitionSaver<'a> {
    pub fn new(bindings: &'a BindingArena, string_view: Option<BindingId>) -> Self {
        Self {
            bindings,
            kept: string_view.into_iter().collect(),
            encoded: Vec::new(),
        }
    }

    pub fn take_from(&mut self, module: &mut CoreModule) {
        let items = std::mem::take(&mut module.items);
        self.kept
            .extend(items.iter().flat_map(|item| item.kind.method_bodies()));
        let saved = self.saved_bindings(&items);
        for (position, item) in items.into_iter().enumerate() {
            match Self::named_binding(&item).filter(|binding| saved.contains(binding)) {
                Some(binding) => self.encode(binding, module.id, position, &item),
                None => module.items.push(item),
            }
        }
    }

    fn saved_bindings(&self, items: &[Item]) -> FxHashSet<BindingId> {
        let definitions = items.iter().filter_map(|item| match item.kind {
            ItemKind::Def(binding, _) => Some(binding),
            _ => None,
        });
        definitions
            .filter(|binding| !self.kept.contains(binding))
            .collect()
    }

    fn named_binding(item: &Item) -> Option<BindingId> {
        match item.kind {
            ItemKind::Def(binding, _) | ItemKind::Signature(binding, _) => Some(binding),
            _ => None,
        }
    }

    fn encode(&mut self, binding: BindingId, module: ModuleId, position: usize, item: &Item) {
        self.encoded.push(EncodedItem {
            entry: SavedItem {
                binding: binding.index(),
                module: SavedWords::index(module.index()),
                position: SavedWords::index(position),
                item_start: 0,
                item_end: 0,
                references_start: 0,
                references_end: 0,
            },
            item: bincode::serialize(item).expect("every prelude item serializes"),
            references: self.references_of(item),
        });
    }

    fn references_of(&self, item: &Item) -> Vec<BindingId> {
        match &item.kind {
            ItemKind::Def(_, value) => References::of(value, self.bindings).into_globals(),
            _ => Vec::new(),
        }
    }

    pub fn finish(mut self) -> SavedTables {
        self.encoded
            .sort_by_key(|encoded| (encoded.entry.binding, encoded.entry.position));
        let (mut entries, mut references, mut items) = (Vec::new(), Vec::new(), Vec::new());
        for encoded in self.encoded {
            let mut entry = encoded.entry;
            entry.item_start = SavedWords::index(items.len());
            items.extend(encoded.item);
            entry.item_end = SavedWords::index(items.len());
            entry.references_start = SavedWords::index(references.len());
            references.extend(
                encoded
                    .references
                    .iter()
                    .flat_map(|reference| reference.index().to_le_bytes()),
            );
            entry.references_end = SavedWords::index(references.len());
            entry.write(&mut entries);
        }
        SavedTables {
            entries,
            references,
            items,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustc_hash::FxHashSet;

    use super::{DefinitionSaver, SavedDefinitions};
    use crate::core::{BindingId, ItemKind, ModulePath};
    use crate::expand::{Expander, PreludeState, Program};
    use crate::modules::ModuleSources;
    use crate::source::SourceMap;

    struct BuiltModule {
        program: Program,
    }

    impl BuiltModule {
        const TRAIT_AND_IMPL: &str = "
            (trait named
              (name (fn ((ref self)) i64) (self) 1)
              (rank (fn ((ref self)) i64)))
            (defstruct car :year i64)
            (impl named car
              (rank (c) (:year c)))
            (defn helper () 1)
            (defn main ())";
        const METHOD_BODIES: usize = 2;

        fn expand(text: &str) -> Self {
            let mut sources = SourceMap::default();
            let entry = sources.add(LoadedProgram::FILE, text);
            let module_sources = ModuleSources::new(PathBuf::new(), PathBuf::new());
            let program = Expander::new(&mut sources, module_sources)
                .expand(ModulePath::new(LoadedProgram::MODULE), entry)
                .unwrap_or_else(|_| panic!("the module expands"));
            Self { program }
        }

        fn method_bodies(&self) -> Vec<BindingId> {
            let items = &self.program.entry_module().items;
            items
                .iter()
                .flat_map(|item| item.kind.method_bodies())
                .collect()
        }

        fn definition(&self, name: &str) -> BindingId {
            let program = &self.program;
            let found = program
                .entry_module()
                .definitions()
                .map(|(binding, _)| binding);
            let mut found = found.filter(|binding| program.bindings.get(*binding).name() == name);
            found.next().expect("the module defines the name")
        }

        fn defines(&self, binding: BindingId) -> bool {
            let mut definitions = self.program.entry_module().definitions();
            definitions.any(|(defined, _)| defined == binding)
        }

        fn save_definitions(&mut self) -> SavedDefinitions {
            let entry = self.program.entry;
            let mut saver = DefinitionSaver::new(&self.program.bindings, None);
            let module = self
                .program
                .modules
                .iter_mut()
                .find(|module| module.id == entry);
            saver.take_from(module.expect("the entry module is expanded"));
            let encoded = bincode::serialize(&saver.finish()).expect("the tables serialize");
            bincode::deserialize(encoded.leak()).expect("the tables load")
        }

        fn saves(saved: &SavedDefinitions, binding: BindingId) -> bool {
            saved.entries_of(binding).next().is_some()
        }
    }

    struct LoadedProgram;

    impl LoadedProgram {
        const MODULE: &str = "program";
        const FILE: &str = "program.rslisp";
        const EMPTY: &str = "(defn main ())";
        const SUMMING: &str = "(defn main () (println (sum (vec 1 2))))";
        const MACRO_HELPER: &str = "(use code rest)
            (use prelude/macros map-list)
            (def one (macro (form) (do (map-list (fn (item) item) (rest form)) 1)))
            (defn main () (println (one a b)))";

        fn expand(text: &str) -> Program {
            let state = PreludeState::load(PreludeState::build().save().leak())
                .expect("the saved prelude loads");
            let mut sources = SourceMap::default();
            let entry = sources.add(Self::FILE, text);
            let module_sources = ModuleSources::new(PathBuf::new(), PathBuf::new());
            Expander::with_prelude(&mut sources, module_sources, state)
                .expand(ModulePath::new(Self::MODULE), entry)
                .unwrap_or_else(|_| panic!("the program expands"))
        }

        fn holds(program: &Program, name: &str, named: fn(&ItemKind) -> Option<BindingId>) -> bool {
            program
                .modules
                .iter()
                .flat_map(|module| &module.items)
                .filter_map(|item| named(&item.kind))
                .any(|binding| program.bindings.get(binding).name() == name)
        }

        fn holds_definition(program: &Program, name: &str) -> bool {
            Self::holds(program, name, |kind| match kind {
                ItemKind::Def(binding, _) => Some(*binding),
                _ => None,
            })
        }

        fn holds_signature(program: &Program, name: &str) -> bool {
            Self::holds(program, name, |kind| match kind {
                ItemKind::Signature(binding, _) => Some(*binding),
                _ => None,
            })
        }

        fn undecoded_names(program: &Program) -> FxHashSet<String> {
            let names = program
                .pending_definitions
                .undecoded()
                .map(|binding| program.bindings.get(binding).name().to_string());
            names.collect()
        }

        fn defined_binding(program: &Program, name: &str) -> BindingId {
            program
                .modules
                .iter()
                .flat_map(|module| module.definitions())
                .map(|(binding, _)| binding)
                .find(|binding| program.bindings.get(*binding).name() == name)
                .expect("the program holds the definition")
        }

        fn binding_of(program: &Program, name: &str) -> BindingId {
            program
                .pending_definitions
                .undecoded()
                .find(|binding| program.bindings.get(*binding).name() == name)
                .expect("the name is undecoded")
        }
    }

    #[test]
    fn a_program_decodes_no_definition_it_does_not_reach() {
        let program = LoadedProgram::expand(LoadedProgram::EMPTY);
        assert!(!LoadedProgram::holds_definition(&program, "sum"));
        assert!(LoadedProgram::undecoded_names(&program).contains("sum"));
    }

    #[test]
    fn a_reached_definition_returns_with_what_it_reaches() {
        let program = LoadedProgram::expand(LoadedProgram::SUMMING);
        assert!(LoadedProgram::holds_definition(&program, "sum"));
        assert!(LoadedProgram::holds_definition(&program, "add"));
    }

    #[test]
    fn a_reached_definition_returns_with_its_signature() {
        let program = LoadedProgram::expand(LoadedProgram::SUMMING);
        assert!(LoadedProgram::holds_signature(&program, "sum"));
    }

    #[test]
    fn a_restored_definition_leaves_the_undecoded_names() {
        let program = LoadedProgram::expand(LoadedProgram::SUMMING);
        assert!(!LoadedProgram::undecoded_names(&program).contains("sum"));
    }

    #[test]
    fn a_checked_root_returns_though_nothing_reaches_it() {
        let mut program = LoadedProgram::expand(LoadedProgram::EMPTY);
        let sum = LoadedProgram::binding_of(&program, "sum");
        program.check_also(sum);
        assert!(LoadedProgram::holds_definition(&program, "sum"));
    }

    #[test]
    fn a_restore_keeps_the_order_of_its_module_within_one_batch() {
        let program = LoadedProgram::expand(LoadedProgram::SUMMING);
        let names: Vec<&str> = program
            .modules
            .iter()
            .flat_map(|module| module.definitions())
            .map(|(binding, _)| program.bindings.get(binding).name())
            .collect();
        let position = |name| {
            let found = names.iter().position(|found| *found == name);
            found.expect("the program holds the definition")
        };
        assert!(position("fold-items") < position("sum"));
    }

    #[test]
    fn a_definition_returns_once() {
        let mut program = LoadedProgram::expand(LoadedProgram::SUMMING);
        let sum = LoadedProgram::defined_binding(&program, "sum");
        program.check_also(sum);
        let copies = program
            .modules
            .iter()
            .flat_map(|module| module.definitions())
            .filter(|(binding, _)| *binding == sum)
            .count();
        assert_eq!(copies, 1);
    }

    #[test]
    fn an_unreached_signature_stays_undecoded() {
        let program = LoadedProgram::expand(LoadedProgram::EMPTY);
        assert!(!LoadedProgram::holds_signature(&program, "sum"));
    }

    #[test]
    fn a_saver_keeps_every_method_body_in_its_module() {
        let mut built = BuiltModule::expand(BuiltModule::TRAIT_AND_IMPL);
        let methods = built.method_bodies();
        assert_eq!(methods.len(), BuiltModule::METHOD_BODIES);
        let saved = built.save_definitions();
        for method in methods {
            assert!(built.defines(method));
            assert!(!BuiltModule::saves(&saved, method));
        }
    }

    #[test]
    fn a_saver_saves_a_plain_definition() {
        let mut built = BuiltModule::expand(BuiltModule::TRAIT_AND_IMPL);
        let helper = built.definition("helper");
        let saved = built.save_definitions();
        assert!(!built.defines(helper));
        assert!(BuiltModule::saves(&saved, helper));
    }

    #[test]
    fn the_string_view_stays_decoded() {
        let program = LoadedProgram::expand(LoadedProgram::EMPTY);
        let view = program.string_view.expect("the prelude views strings");
        assert!(
            !program
                .pending_definitions
                .undecoded()
                .any(|bound| bound == view)
        );
    }

    #[test]
    fn a_macro_runs_a_saved_prelude_function_at_compile_time() {
        let program = LoadedProgram::expand(LoadedProgram::MACRO_HELPER);
        assert!(!LoadedProgram::holds_definition(&program, "map-list"));
    }

    #[test]
    fn no_saved_definitions_hold_no_value() {
        let unknown = BindingId::from_index(0);
        assert!(SavedDefinitions::default().value_of(unknown).is_none());
    }
}

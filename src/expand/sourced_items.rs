pub struct SourcedItems<T> {
    sources: usize,
    items: Vec<SourcedItem<T>>,
}

struct SourcedItem<T> {
    source: usize,
    item: T,
}

pub struct SourceRun<T> {
    pub source: usize,
    pub items: Vec<T>,
}

impl<T> SourcedItems<T> {
    pub fn new() -> Self {
        Self {
            sources: 0,
            items: Vec::new(),
        }
    }

    pub fn add_source(&mut self, items: Vec<T>) {
        let source = self.sources;
        self.sources += 1;
        self.items
            .extend(items.into_iter().map(|item| SourcedItem { source, item }));
    }

    pub fn partition(self, belongs: impl Fn(&T) -> bool) -> (Self, Self) {
        let sources = self.sources;
        let (matching, rest) = self
            .items
            .into_iter()
            .partition(|sourced| belongs(&sourced.item));
        (Self::of(sources, matching), Self::of(sources, rest))
    }

    pub fn into_runs(self) -> Vec<SourceRun<T>> {
        let mut runs: Vec<SourceRun<T>> = Vec::new();
        for SourcedItem { source, item } in self.items {
            match runs.last_mut() {
                Some(run) if run.source == source => run.items.push(item),
                _ => runs.push(SourceRun {
                    source,
                    items: vec![item],
                }),
            }
        }
        runs
    }

    fn of(sources: usize, items: Vec<SourcedItem<T>>) -> Self {
        Self { sources, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RunsFixture {
        items: SourcedItems<&'static str>,
    }

    impl RunsFixture {
        fn of_sources(sources: &[&[&'static str]]) -> Self {
            let mut items = SourcedItems::new();
            for source in sources {
                items.add_source(source.to_vec());
            }
            Self { items }
        }

        fn runs(self) -> Vec<(usize, Vec<&'static str>)> {
            self.items
                .into_runs()
                .into_iter()
                .map(|run| (run.source, run.items))
                .collect()
        }
    }

    #[test]
    fn groups_the_items_of_each_source_into_one_run() {
        let fixture = RunsFixture::of_sources(&[&["a", "b"], &[], &["c"]]);
        assert_eq!(fixture.runs(), vec![(0, vec!["a", "b"]), (2, vec!["c"])]);
    }

    #[test]
    fn keeps_each_item_with_its_source_through_a_partition() {
        let fixture = RunsFixture::of_sources(&[&["impl", "def"], &["impl"]]);
        let (impls, _) = fixture.items.partition(|item| *item == "impl");
        let runs = RunsFixture { items: impls }.runs();
        assert_eq!(runs, vec![(0, vec!["impl"]), (1, vec!["impl"])]);
    }
}

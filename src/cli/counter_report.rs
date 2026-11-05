use serde_json::{Map, Value};

use super::stage_work::StageWork;
use super::standard_error::StandardError;
use crate::counters::{AllocationCounts, CountingAllocator};

pub struct CounterReport {
    counters: Map<String, Value>,
}

impl CounterReport {
    const ALLOCATOR_GROUP: &'static str = "allocator";

    pub fn new(allocator: Option<&CountingAllocator>) -> Self {
        let mut report = Self {
            counters: Map::new(),
        };
        if let Some(allocator) = allocator {
            report.add_allocation_counts(allocator.counts());
        }
        report
    }

    pub fn add_stage_work(&mut self, work: &StageWork) {
        self.add_stage_count("expand", "lowered_forms", work.lowered_forms);
        self.add_stage_count("check", "inferred_expressions", work.inferred_expressions);
        self.add_stage_count("emit", "emitted_expressions", work.emitted_expressions);
        self.add_stage_count("build", "scanned_tokens", work.scanned_tokens);
    }

    pub fn print(self) {
        StandardError::write_line(self.into_json());
    }

    fn add_allocation_counts(&mut self, counts: AllocationCounts) {
        self.add_group(
            Self::ALLOCATOR_GROUP,
            [
                ("allocations", counts.allocations),
                ("allocated_bytes", counts.allocated_bytes),
                ("peak_live_bytes", counts.peak_live_bytes),
            ],
        );
    }

    fn add_stage_count(&mut self, stage: &str, counter: &'static str, count: Option<usize>) {
        self.add_group(stage, count.map(|count| (counter, count)));
    }

    fn add_group(
        &mut self,
        group: &str,
        counters: impl IntoIterator<Item = (&'static str, usize)>,
    ) {
        for (counter, value) in counters {
            self.counters
                .insert(format!("{group}.{counter}"), Value::from(value));
        }
    }

    fn into_json(self) -> Value {
        Value::Object(self.counters)
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::{GlobalAlloc, Layout};

    use serde_json::json;

    use super::*;

    struct AllocationFixture;

    impl AllocationFixture {
        const SIZE: usize = 32;

        fn allocate_once(allocator: &CountingAllocator) {
            let layout =
                Layout::from_size_align(Self::SIZE, align_of::<u64>()).expect("a valid layout");
            unsafe {
                let pointer = allocator.alloc(layout);
                allocator.dealloc(pointer, layout);
            }
        }
    }

    #[test]
    fn a_report_with_an_allocator_holds_its_three_counts() {
        let allocator = CountingAllocator::new();
        AllocationFixture::allocate_once(&allocator);
        assert_eq!(
            CounterReport::new(Some(&allocator)).into_json(),
            json!({
                "allocator.allocations": 1,
                "allocator.allocated_bytes": AllocationFixture::SIZE,
                "allocator.peak_live_bytes": AllocationFixture::SIZE,
            })
        );
    }

    #[test]
    fn a_report_without_an_allocator_holds_no_key() {
        assert_eq!(CounterReport::new(None).into_json(), json!({}));
    }
}

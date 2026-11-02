use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CountingAllocator {
    allocations: AtomicUsize,
    allocated_bytes: AtomicUsize,
    live_bytes: AtomicUsize,
    peak_live_bytes: AtomicUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationCounts {
    pub allocations: usize,
    pub allocated_bytes: usize,
    pub peak_live_bytes: usize,
}

impl CountingAllocator {
    pub const fn new() -> Self {
        Self {
            allocations: AtomicUsize::new(0),
            allocated_bytes: AtomicUsize::new(0),
            live_bytes: AtomicUsize::new(0),
            peak_live_bytes: AtomicUsize::new(0),
        }
    }

    pub fn counts(&self) -> AllocationCounts {
        AllocationCounts {
            allocations: self.allocations.load(Ordering::Relaxed),
            allocated_bytes: self.allocated_bytes.load(Ordering::Relaxed),
            peak_live_bytes: self.peak_live_bytes.load(Ordering::Relaxed),
        }
    }

    fn recorded(&self, pointer: *mut u8, size: usize) -> *mut u8 {
        if !pointer.is_null() {
            self.record_allocation(size);
        }
        pointer
    }

    fn record_allocation(&self, size: usize) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        self.allocated_bytes.fetch_add(size, Ordering::Relaxed);
        let live_bytes = self.live_bytes.fetch_add(size, Ordering::Relaxed) + size;
        self.peak_live_bytes
            .fetch_max(live_bytes, Ordering::Relaxed);
    }

    fn record_release(&self, size: usize) {
        self.live_bytes.fetch_sub(size, Ordering::Relaxed);
    }
}

impl Default for CountingAllocator {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.recorded(unsafe { System.alloc(layout) }, layout.size())
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        self.recorded(unsafe { System.alloc_zeroed(layout) }, layout.size())
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        self.record_release(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            self.record_release(layout.size());
            self.record_allocation(new_size);
        }
        resized
    }
}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::*;

    struct AllocatorFixture {
        allocator: CountingAllocator,
    }

    impl AllocatorFixture {
        const SMALL: usize = 16;
        const LARGE: usize = 64;

        fn new() -> Self {
            Self {
                allocator: CountingAllocator::new(),
            }
        }

        fn layout(size: usize) -> Layout {
            Layout::from_size_align(size, align_of::<u64>()).expect("a valid layout")
        }

        fn allocate(&self, size: usize) -> *mut u8 {
            unsafe { self.allocator.alloc(Self::layout(size)) }
        }

        fn allocate_zeroed(&self, size: usize) -> *mut u8 {
            unsafe { self.allocator.alloc_zeroed(Self::layout(size)) }
        }

        fn holds_only_zeros(pointer: *const u8, size: usize) -> bool {
            unsafe { slice::from_raw_parts(pointer, size) }
                .iter()
                .all(|byte| *byte == 0)
        }

        fn release(&self, pointer: *mut u8, size: usize) {
            unsafe { self.allocator.dealloc(pointer, Self::layout(size)) }
        }

        fn resize(&self, pointer: *mut u8, size: usize, new_size: usize) -> *mut u8 {
            unsafe {
                self.allocator
                    .realloc(pointer, Self::layout(size), new_size)
            }
        }
    }

    #[test]
    fn each_allocation_adds_its_bytes() {
        let fixture = AllocatorFixture::new();
        let small = fixture.allocate(AllocatorFixture::SMALL);
        let large = fixture.allocate(AllocatorFixture::LARGE);
        fixture.release(small, AllocatorFixture::SMALL);
        fixture.release(large, AllocatorFixture::LARGE);
        assert_eq!(
            fixture.allocator.counts(),
            AllocationCounts {
                allocations: 2,
                allocated_bytes: AllocatorFixture::SMALL + AllocatorFixture::LARGE,
                peak_live_bytes: AllocatorFixture::SMALL + AllocatorFixture::LARGE,
            }
        );
    }

    #[test]
    fn a_release_keeps_the_peak() {
        let fixture = AllocatorFixture::new();
        let large = fixture.allocate(AllocatorFixture::LARGE);
        fixture.release(large, AllocatorFixture::LARGE);
        let small = fixture.allocate(AllocatorFixture::SMALL);
        fixture.release(small, AllocatorFixture::SMALL);
        assert_eq!(
            fixture.allocator.counts().peak_live_bytes,
            AllocatorFixture::LARGE
        );
    }

    #[test]
    fn a_resize_counts_as_one_allocation_of_the_new_size() {
        let fixture = AllocatorFixture::new();
        let small = fixture.allocate(AllocatorFixture::SMALL);
        let large = fixture.resize(small, AllocatorFixture::SMALL, AllocatorFixture::LARGE);
        fixture.release(large, AllocatorFixture::LARGE);
        assert_eq!(
            fixture.allocator.counts(),
            AllocationCounts {
                allocations: 2,
                allocated_bytes: AllocatorFixture::SMALL + AllocatorFixture::LARGE,
                peak_live_bytes: AllocatorFixture::LARGE,
            }
        );
    }

    #[test]
    fn a_zeroed_allocation_counts_like_any_other() {
        let fixture = AllocatorFixture::new();
        let small = fixture.allocate_zeroed(AllocatorFixture::SMALL);
        assert_eq!(
            fixture.allocator.counts(),
            AllocationCounts {
                allocations: 1,
                allocated_bytes: AllocatorFixture::SMALL,
                peak_live_bytes: AllocatorFixture::SMALL,
            }
        );
        assert!(AllocatorFixture::holds_only_zeros(
            small,
            AllocatorFixture::SMALL
        ));
        fixture.release(small, AllocatorFixture::SMALL);
    }
}

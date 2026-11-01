use std::cell::Cell;

pub struct DeepRecursion;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl DeepRecursion {
    const LEVELS_PER_CHECK: usize = 8;
    const LARGEST_LEVEL: usize = 32 * 1024;
    const INNERMOST_WORK: usize = 64 * 1024;
    const RED_ZONE: usize =
        Self::INNERMOST_WORK + (Self::LEVELS_PER_CHECK - 1) * Self::LARGEST_LEVEL;
    const GROWTH: usize = 1024 * 1024;

    pub fn guard<R>(work: impl FnOnce() -> R) -> R {
        let level = Level::enter();
        if level.is_checked() {
            stacker::maybe_grow(Self::RED_ZONE, Self::GROWTH, work)
        } else {
            work()
        }
    }
}

struct Level {
    depth: usize,
}

impl Level {
    fn enter() -> Self {
        let depth = DEPTH.get() + 1;
        DEPTH.set(depth);
        Self { depth }
    }

    fn is_checked(&self) -> bool {
        self.depth.is_multiple_of(DeepRecursion::LEVELS_PER_CHECK)
    }
}

impl Drop for Level {
    fn drop(&mut self) {
        DEPTH.set(self.depth - 1);
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::DeepRecursion;

    struct ShortStack;

    impl ShortStack {
        const SIZE: usize = DeepRecursion::RED_ZONE / 2;
        const UNCHECKED_LEVELS: usize = DeepRecursion::LEVELS_PER_CHECK - 1;

        fn run<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
            thread::Builder::new()
                .stack_size(Self::SIZE)
                .spawn(work)
                .expect("the test thread starts")
                .join()
                .expect("the test thread finishes")
        }

        fn remaining_inside(levels: usize) -> usize {
            if levels == 0 {
                stacker::remaining_stack().expect("the platform reports its stack limit")
            } else {
                DeepRecursion::guard(|| Self::remaining_inside(levels - 1))
            }
        }
    }

    #[test]
    fn levels_between_checks_run_on_the_stack_they_find() {
        let remaining =
            ShortStack::run(|| ShortStack::remaining_inside(ShortStack::UNCHECKED_LEVELS));
        assert!(remaining < DeepRecursion::RED_ZONE);
    }

    #[test]
    fn a_checked_level_grows_a_short_stack() {
        let remaining =
            ShortStack::run(|| ShortStack::remaining_inside(DeepRecursion::LEVELS_PER_CHECK));
        assert!(remaining >= DeepRecursion::RED_ZONE);
    }

    #[test]
    fn returning_guards_give_their_levels_back() {
        let remaining = ShortStack::run(|| {
            ShortStack::remaining_inside(ShortStack::UNCHECKED_LEVELS);
            ShortStack::remaining_inside(ShortStack::UNCHECKED_LEVELS)
        });
        assert!(remaining < DeepRecursion::RED_ZONE);
    }
}

pub trait Sequence {
    fn len(&self) -> usize;
}

impl Sequence for String {
    fn len(&self) -> usize {
        self.as_str().len()
    }
}

impl Sequence for str {
    fn len(&self) -> usize {
        str::len(self)
    }
}

impl<T> Sequence for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl<T> Sequence for [T] {
    fn len(&self) -> usize {
        <[T]>::len(self)
    }
}

impl<T: Sequence + ?Sized> Sequence for &T {
    fn len(&self) -> usize {
        T::len(self)
    }
}

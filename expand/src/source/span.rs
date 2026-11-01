use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SourceId(u32);

impl SourceId {
    pub fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 source files"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Span {
    source: SourceId,
    start: u32,
    end: u32,
}

impl Span {
    pub fn new(source: SourceId, range: Range<usize>) -> Self {
        Self {
            source,
            start: Self::offset(range.start),
            end: Self::offset(range.end),
        }
    }

    pub fn source(self) -> SourceId {
        self.source
    }

    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn contains(self, inner: Span) -> bool {
        self.source == inner.source && self.start <= inner.start && inner.end <= self.end
    }

    fn offset(position: usize) -> u32 {
        u32::try_from(position).expect("source files are smaller than 4 GiB")
    }
}

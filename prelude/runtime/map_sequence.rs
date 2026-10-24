use std::collections::BTreeMap;

impl<Key, Value> Sequence for BTreeMap<Key, Value> {
    fn len(&self) -> usize {
        BTreeMap::len(self)
    }
}

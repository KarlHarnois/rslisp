use std::collections::{BTreeMap, HashMap};

impl<Key, Value> Sequence for BTreeMap<Key, Value> {
    fn len(&self) -> usize {
        BTreeMap::len(self)
    }
}

impl<Key, Value> Sequence for HashMap<Key, Value> {
    fn len(&self) -> usize {
        HashMap::len(self)
    }
}

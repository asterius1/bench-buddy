//! A small map that keeps insertion order, like a Python dict: the judging iterates some maps and
//! the order decides ties.

#[derive(Clone, Debug)]
pub struct OMap<K, V>(pub Vec<(K, V)>);

impl<K, V> Default for OMap<K, V> {
    fn default() -> Self {
        OMap(Vec::new())
    }
}

impl<K: PartialEq, V> OMap<K, V> {
    pub fn get(&self, k: &K) -> Option<&V> {
        self.0.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.0.iter_mut().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    pub fn contains(&self, k: &K) -> bool {
        self.get(k).is_some()
    }
    /// Sets the value; a key already there keeps its place.
    pub fn insert(&mut self, k: K, v: V) {
        match self.0.iter().position(|(x, _)| *x == k) {
            Some(i) => self.0[i].1 = v,
            None => self.0.push((k, v)),
        }
    }
    pub fn remove(&mut self, k: &K) -> Option<V> {
        let i = self.0.iter().position(|(x, _)| x == k)?;
        Some(self.0.remove(i).1)
    }
    /// The value for k, put in with `v` first (dict.setdefault).
    pub fn entry_or(&mut self, k: K, v: V) -> &mut V {
        let i = match self.0.iter().position(|(x, _)| *x == k) {
            Some(i) => i,
            None => {
                self.0.push((k, v));
                self.0.len() - 1
            }
        };
        &mut self.0[i].1
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: PartialEq, V: Default> OMap<K, V> {
    pub fn entry(&mut self, k: K) -> &mut V {
        self.entry_or(k, V::default())
    }
}

impl<K: PartialEq, V> FromIterator<(K, V)> for OMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(it: I) -> Self {
        let mut m = OMap::default();
        for (k, v) in it {
            m.insert(k, v);
        }
        m
    }
}

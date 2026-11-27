use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Entries are ordered from least to most recently used.
pub(crate) fn take_first<T>(
    entries: &mut VecDeque<T>,
    matches: impl FnMut(&T) -> bool,
) -> Option<T> {
    let index = entries.iter().position(matches)?;
    entries.remove(index)
}

/// Returns the remaining weight. Callers choose which in-use entries may be reclaimed.
pub(crate) fn trim<T>(
    entries: &mut VecDeque<T>,
    max_entries: usize,
    mut bytes: u64,
    max_bytes: u64,
    mut evictable: impl FnMut(&T) -> bool,
    weight: impl Fn(&T) -> u64,
) -> u64 {
    let mut evicted = false;
    while entries.len() > max_entries || bytes > max_bytes {
        let Some(entry) = take_first(entries, &mut evictable) else {
            break;
        };
        bytes = bytes.saturating_sub(weight(&entry));
        evicted = true;
    }
    if evicted {
        if entries.capacity() > entries.len().saturating_mul(4).max(16) {
            entries.shrink_to_fit();
        }
        crate::note_cache_eviction();
    }
    bytes
}

pub(crate) struct ByteBoundedLru<V> {
    inner: Mutex<(VecDeque<V>, u64)>,
    max_entries: usize,
    max_bytes: u64,
}

impl<V> ByteBoundedLru<V> {
    pub(crate) const fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            inner: Mutex::new((VecDeque::new(), 0)),
            max_entries,
            max_bytes,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&VecDeque<V>, u64) -> R) -> R {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard.0, guard.1)
    }

    pub(crate) fn take(
        &self,
        matches: impl FnMut(&V) -> bool,
        weight: impl Fn(&V) -> u64,
    ) -> Option<V> {
        let mut guard = self.inner.lock().ok()?;
        let found = take_first(&mut guard.0, matches)?;
        guard.1 = guard.1.saturating_sub(weight(&found));
        Some(found)
    }

    pub(crate) fn insert(
        &self,
        item: V,
        mut replace: impl FnMut(&V) -> bool,
        weight: impl Fn(&V) -> u64,
    ) {
        let item_bytes = weight(&item);
        if item_bytes > self.max_bytes {
            return;
        }
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        let (entries, bytes) = &mut *guard;
        while let Some(old) = take_first(entries, &mut replace) {
            *bytes = bytes.saturating_sub(weight(&old));
        }
        let total = bytes.saturating_add(item_bytes);
        entries.push_back(item);
        *bytes = trim(
            entries,
            self.max_entries,
            total,
            self.max_bytes,
            |_| true,
            &weight,
        );
    }

    pub(crate) fn evict_idle(
        &self,
        max_idle: Duration,
        last_accessed: impl Fn(&V) -> Instant,
        weight: impl Fn(&V) -> u64,
    ) -> usize {
        let Ok(mut guard) = self.inner.lock() else {
            return 0;
        };
        let (entries, bytes) = &mut *guard;
        let before = entries.len();
        let now = Instant::now();
        let mut removed_bytes = 0u64;
        entries.retain(|entry| {
            let keep = now.saturating_duration_since(last_accessed(entry)) < max_idle;
            if !keep {
                removed_bytes = removed_bytes.saturating_add(weight(entry));
            }
            keep
        });
        *bytes = bytes.saturating_sub(removed_bytes);
        let evicted = before - entries.len();
        if evicted > 0 {
            if entries.capacity() > entries.len().saturating_mul(4).max(16) {
                entries.shrink_to_fit();
            }
            crate::note_cache_eviction();
        }
        evicted
    }
}

struct ArenaNode<K, V> {
    key: K,
    value: V,
    accessed: Instant,
    prev: Option<usize>,
    next: Option<usize>,
}

/// O(1) arena-backed doubly-linked LRU map without heap node allocations per touch.
pub(crate) struct BoundedLruMap<K, V> {
    max_entries: usize,
    index: HashMap<K, usize>,
    nodes: Vec<Option<ArenaNode<K, V>>>,
    free: Vec<usize>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl<K: Clone + Eq + Hash, V: Clone> BoundedLruMap<K, V> {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            index: HashMap::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            head: None,
            tail: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.index.len()
    }

    fn detach(&mut self, idx: usize) {
        let (prev, next) = {
            let Some(node) = self.nodes[idx].as_ref() else {
                return;
            };
            (node.prev, node.next)
        };
        if let Some(p) = prev {
            if let Some(pn) = self.nodes[p].as_mut() {
                pn.next = next;
            }
        } else {
            self.head = next;
        }
        if let Some(n) = next {
            if let Some(nn) = self.nodes[n].as_mut() {
                nn.prev = prev;
            }
        } else {
            self.tail = prev;
        }
        if let Some(node) = self.nodes[idx].as_mut() {
            node.prev = None;
            node.next = None;
        }
    }

    fn attach_tail(&mut self, idx: usize) {
        let old_tail = self.tail;
        if let Some(node) = self.nodes[idx].as_mut() {
            node.prev = old_tail;
            node.next = None;
        }
        if let Some(t) = old_tail {
            if let Some(tn) = self.nodes[t].as_mut() {
                tn.next = Some(idx);
            }
        } else {
            self.head = Some(idx);
        }
        self.tail = Some(idx);
    }

    fn pop_head(&mut self) -> Option<ArenaNode<K, V>> {
        let h = self.head?;
        self.detach(h);
        let removed = self.nodes[h].take()?;
        self.index.remove(&removed.key);
        self.free.push(h);
        Some(removed)
    }

    #[cfg(test)]
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let idx = self.index.remove(key)?;
        self.detach(idx);
        let removed = self.nodes[idx].take()?;
        self.free.push(idx);
        Some(removed.value)
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        let &idx = self.index.get(key)?;
        let now = Instant::now();
        let value = {
            let node = self.nodes[idx].as_mut()?;
            node.accessed = now;
            node.value.clone()
        };
        if self.tail != Some(idx) {
            self.detach(idx);
            self.attach_tail(idx);
        }
        Some(value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.insert_at(key, value, Instant::now());
    }

    pub(crate) fn insert_at(&mut self, key: K, value: V, now: Instant) {
        if let Some(&idx) = self.index.get(&key) {
            if let Some(node) = self.nodes[idx].as_mut() {
                node.value = value;
                node.accessed = now;
            }
            if self.tail != Some(idx) {
                self.detach(idx);
                self.attach_tail(idx);
            }
            return;
        }
        if self.max_entries == 0 {
            return;
        }
        if self.index.len() >= self.max_entries {
            let _ = self.pop_head();
        }
        let idx = if let Some(slot) = self.free.pop() {
            self.nodes[slot] = Some(ArenaNode {
                key: key.clone(),
                value,
                accessed: now,
                prev: None,
                next: None,
            });
            slot
        } else {
            let slot = self.nodes.len();
            self.nodes.push(Some(ArenaNode {
                key: key.clone(),
                value,
                accessed: now,
                prev: None,
                next: None,
            }));
            slot
        };
        self.index.insert(key, idx);
        self.attach_tail(idx);
    }

    pub(crate) fn evict_idle(&mut self, max_idle: Duration) -> bool {
        self.evict_idle_at(Instant::now(), max_idle)
    }

    pub(crate) fn evict_idle_at(&mut self, now: Instant, max_idle: Duration) -> bool {
        let before = self.index.len();
        while let Some(h) = self.head {
            let expired = self.nodes[h]
                .as_ref()
                .is_some_and(|n| now.saturating_duration_since(n.accessed) >= max_idle);
            if expired {
                let _ = self.pop_head();
            } else {
                break;
            }
        }
        if self.index.len() < before {
            if self.nodes.capacity() > self.index.len().saturating_mul(4).max(16) {
                let mut compacted = Vec::with_capacity(self.index.len());
                self.index.clear();
                self.free.clear();
                let mut cur = self.head;
                while let Some(idx) = cur {
                    let mut node = self.nodes[idx].take().expect("active arena node");
                    cur = node.next;
                    let new_idx = compacted.len();
                    node.prev = new_idx.checked_sub(1);
                    node.next = cur.map(|_| new_idx + 1);
                    self.index.insert(node.key.clone(), new_idx);
                    compacted.push(Some(node));
                }
                self.head = (!compacted.is_empty()).then_some(0);
                self.tail = compacted.len().checked_sub(1);
                self.nodes = compacted;
                self.nodes.shrink_to_fit();
                self.free.shrink_to_fit();
                self.index.shrink_to_fit();
            }
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_invariants(&self)
    where
        K: std::fmt::Debug,
    {
        let mut seen = Vec::new();
        let mut prev = None;
        let mut cur = self.head;
        while let Some(idx) = cur {
            let node = self.nodes[idx].as_ref().expect("linked slot must be Some");
            assert_eq!(node.prev, prev, "broken prev link at {idx}");
            assert_eq!(self.index.get(&node.key), Some(&idx), "index mismatch");
            prev = Some(idx);
            seen.push(idx);
            cur = node.next;
        }
        assert_eq!(self.tail, prev, "tail mismatch");
        assert_eq!(seen.len(), self.index.len(), "list length != index length");
        let live_slots = self.nodes.iter().filter(|n| n.is_some()).count();
        assert_eq!(live_slots, self.index.len(), "leaked Some slot in arena");
        assert_eq!(
            live_slots + self.free.len(),
            self.nodes.len(),
            "free list + live slots != arena len"
        );
    }
}

use std::collections::VecDeque;

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
    while entries.len() > max_entries || bytes > max_bytes {
        let Some(entry) = take_first(entries, &mut evictable) else {
            break;
        };
        bytes = bytes.saturating_sub(weight(&entry));
    }
    bytes
}

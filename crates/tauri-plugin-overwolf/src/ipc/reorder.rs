//! The reorder buffer behind per-sender (C.3) and per-target (C.5) ordering.
//!
//! Items carry a sequence number starting at 1. An item is released when
//! every lower number has been released or skipped. A number can be skipped
//! explicitly (`ipc_skip`) or, as a fallback, when the buffer has waited for
//! it for longer than the gap timeout.
//!
//! ```
//! use tauri_plugin_overwolf::ipc::reorder::Reorder;
//! let mut r = Reorder::new(64);
//! assert!(r.insert(2, "b", 0).unwrap().is_empty());
//! assert_eq!(r.insert(1, "a", 0).unwrap(), vec!["a", "b"]);
//! assert!(r.insert(4, "d", 0).unwrap().is_empty());
//! assert_eq!(r.skip(3, 0), vec!["d"]);
//! ```

use std::collections::{BTreeMap, BTreeSet};

/// Why an item was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderError {
    /// 0 is not a valid sequence number.
    Zero,
    /// The number was already released, held or skipped.
    Duplicate,
    /// The number is further ahead than the buffer's window.
    TooFarAhead,
}

/// What a gap expiry did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expired<T> {
    /// Items released by skipping the gap.
    pub released: Vec<T>,
    /// The sequence numbers given up on, as inclusive ranges.
    pub gaps: Vec<(u64, u64)>,
}

/// A reorder buffer.
#[derive(Debug, Clone)]
pub struct Reorder<T> {
    next: u64,
    window: u64,
    held: BTreeMap<u64, T>,
    skipped: BTreeSet<u64>,
    waiting_since: Option<u64>,
}

impl<T> Reorder<T> {
    /// An empty buffer expecting 1. `window` bounds how far ahead of the next
    /// expected number an item (or a skip) may be.
    #[must_use]
    pub fn new(window: u64) -> Self {
        Reorder {
            next: 1,
            window: window.max(1),
            held: BTreeMap::new(),
            skipped: BTreeSet::new(),
            waiting_since: None,
        }
    }

    /// The next number expected.
    #[must_use]
    pub fn next(&self) -> u64 {
        self.next
    }

    /// Number of items held back.
    #[must_use]
    pub fn held_len(&self) -> usize {
        self.held.len()
    }

    /// Whether `seq` would be accepted.
    ///
    /// # Errors
    ///
    /// The reason it would be refused.
    pub fn check(&self, seq: u64) -> Result<(), ReorderError> {
        if seq == 0 {
            Err(ReorderError::Zero)
        } else if seq < self.next || self.held.contains_key(&seq) || self.skipped.contains(&seq) {
            Err(ReorderError::Duplicate)
        } else if seq - self.next >= self.window {
            Err(ReorderError::TooFarAhead)
        } else {
            Ok(())
        }
    }

    /// Inserts an item; returns the items now released, in order.
    ///
    /// # Errors
    ///
    /// See [`Reorder::check`]; the item is dropped.
    pub fn insert(&mut self, seq: u64, item: T, now: u64) -> Result<Vec<T>, ReorderError> {
        self.check(seq)?;
        self.held.insert(seq, item);
        Ok(self.drain(now))
    }

    /// Marks `seq` as never coming; returns the items now released. Numbers
    /// already released, out of window, or 0 are ignored.
    pub fn skip(&mut self, seq: u64, now: u64) -> Vec<T> {
        if self.check(seq).is_ok() {
            self.skipped.insert(seq);
        }
        self.drain(now)
    }

    fn drain(&mut self, now: u64) -> Vec<T> {
        let mut out = Vec::new();
        loop {
            if let Some(item) = self.held.remove(&self.next) {
                out.push(item);
            } else if !self.skipped.remove(&self.next) {
                break;
            }
            self.next += 1;
        }
        if self.held.is_empty() {
            self.waiting_since = None;
            // Skips ahead of `next` with nothing held stay until reached.
        } else if !out.is_empty() || self.waiting_since.is_none() {
            self.waiting_since = Some(now);
        }
        out
    }

    /// Gives up on missing numbers the buffer has waited for at least
    /// `gap_ms`, releasing what follows them.
    pub fn expire(&mut self, now: u64, gap_ms: u64) -> Expired<T> {
        let mut released = Vec::new();
        let mut gaps = Vec::new();
        while let Some(since) = self.waiting_since {
            if now.saturating_sub(since) < gap_ms {
                break;
            }
            let Some(&first) = self.held.keys().next() else {
                self.waiting_since = None;
                break;
            };
            if first > self.next {
                gaps.push((self.next, first - 1));
            }
            self.skipped.retain(|s| *s > first);
            self.next = first;
            // Each round releases at least `first`, and a new gap starts now.
            released.extend(self.drain(now));
        }
        Expired { released, gaps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_and_bounds() {
        let mut r = Reorder::new(4);
        assert_eq!(r.insert(0, 0, 0), Err(ReorderError::Zero));
        assert_eq!(r.insert(1, 1, 0).unwrap(), vec![1]);
        assert_eq!(r.insert(1, 1, 0), Err(ReorderError::Duplicate));
        assert!(r.insert(3, 3, 0).unwrap().is_empty());
        assert_eq!(r.insert(3, 3, 0), Err(ReorderError::Duplicate));
        assert_eq!(r.insert(6, 6, 0), Err(ReorderError::TooFarAhead));
        assert!(r.insert(5, 5, 0).unwrap().is_empty());
        assert_eq!(r.insert(2, 2, 0).unwrap(), vec![2, 3]);
        assert_eq!(r.next(), 4);
        assert_eq!(r.held_len(), 1);
    }

    #[test]
    fn gap_expiry() {
        let mut r = Reorder::new(100);
        assert!(r.insert(3, "c", 10).unwrap().is_empty());
        assert!(r.insert(6, "f", 20).unwrap().is_empty());
        assert!(r.expire(500, 1000).released.is_empty());
        let e = r.expire(1010, 1000);
        assert_eq!(e.released, vec!["c"]);
        assert_eq!(e.gaps, vec![(1, 2)]);
        // A new gap (4..5) starts at 1010.
        assert!(r.expire(1500, 1000).released.is_empty());
        let e = r.expire(2010, 1000);
        assert_eq!(e.released, vec!["f"]);
        assert_eq!(e.gaps, vec![(4, 5)]);
        assert_eq!(r.next(), 7);
        // Late arrivals of given-up numbers are duplicates.
        assert_eq!(r.insert(4, "d", 2020), Err(ReorderError::Duplicate));
    }

    #[test]
    fn skip_ahead_is_kept_until_reached() {
        let mut r = Reorder::new(100);
        assert!(r.skip(2, 0).is_empty());
        assert_eq!(r.insert(1, 'a', 0).unwrap(), vec!['a']);
        assert_eq!(r.next(), 3);
        assert_eq!(r.insert(3, 'c', 0).unwrap(), vec!['c']);
        assert!(
            r.skip(1000, 0).is_empty(),
            "out of window skips are ignored"
        );
    }
}

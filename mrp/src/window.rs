//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::collections::VecDeque;
use std::fmt::Debug;
use std::result::Result;

#[derive(Debug, PartialEq, Clone)]
pub enum WindowError {
    BeforeWindow,
    AfterWindow,
}

/// Data structure to buffer elements indexed between two bounds.
/// These bounds change as contiguous elements are removed from the structure.
/// Wrapper around a RingBuffer to implement window's sliding indexing
#[derive(Debug)]
pub struct BufferWindow<T: Debug> {
    left: u64,
    data: VecDeque<Option<T>>,
}

impl<T: Debug> BufferWindow<T> {
    /// left_bounds must be greater than 0
    pub fn new(max_size: usize, left_bounds: u64) -> Self {
        assert_ne!(left_bounds, 0, "Left bounds must be greater than 0");
        Self {
            left: left_bounds,
            data: VecDeque::with_capacity(max_size),
        }
    }

    fn get_pos(&self, seqnum: u64) -> Result<usize, WindowError> {
        if seqnum < self.left_bounds() {
            return Err(WindowError::BeforeWindow);
        }

        if seqnum > self.right_bounds() {
            return Err(WindowError::AfterWindow);
        }
        Ok((seqnum - self.left) as usize)
    }

    /// Max size of the window
    fn capacity(&self) -> usize {
        self.data.capacity()
    }

    pub fn is_full(&self) -> bool {
        self.data.len() == self.capacity()
    }

    /// the highest seqnum of an element in the window or previously processed
    /// when the window is currently empty, it is left_bounds() - 1
    pub fn max_seen_seqnum(&self) -> u64 {
        self.left + self.data.len() as u64 - 1
    }

    /// Current lowest valid seqnum
    pub fn left_bounds(&self) -> u64 {
        self.left
    }

    /// Current highest valid seqnum
    pub fn right_bounds(&self) -> u64 {
        self.left + (self.capacity() as u64) - 1
    }

    #[cfg(test)]
    /// Gets element if seqnum is in bounds
    fn get(&self, seqnum: u64) -> Option<&T> {
        if let Ok(pos) = self.get_pos(seqnum) {
            self.data.get(pos)?.as_ref()
        } else {
            None
        }
    }

    /// Gets a mutable reference to element if seqnum is in bounds
    pub fn get_mut(&mut self, seqnum: u64) -> Option<&mut T> {
        if let Ok(pos) = self.get_pos(seqnum) {
            self.data.get_mut(pos)?.as_mut()
        } else {
            None
        }
    }

    /// Buffers data in its position in the window if seqnum is contained in the current window.
    /// Returns [WindowError::BeforeWindow] if seqnum is lower than left bounds
    /// Returns [WindowError::AfterWindow] if seqnum is greater than right bounds
    pub fn put(&mut self, seqnum: u64, element: T) -> Result<(), WindowError> {
        let pos = self.get_pos(seqnum)?;
        while self.data.len() <= pos {
            self.data.push_back(None);
        }

        // since seqnum is checked and we guarantee the vec is long enough
        // we will always be able to set a value, but we do a check again
        if let Some(v) = self.data.get_mut(pos) {
            *v = Some(element);
        }
        Ok(())
    }

    /// Drains and returns the contiguous leading elements in the window and new left bounds.
    /// Stops at a seqnum that does not yet contain an element. Slides the window to the right.
    pub fn drain_front(&mut self) -> Option<(u64, Vec<T>)> {
        if self.data.front().unwrap_or(&None).is_none() {
            return None;
        }

        let index = self
            .data
            .iter()
            .position(|e| e.is_none())
            .unwrap_or(self.data.len());
        let elements = self
            .data
            .drain(..index)
            // each element is guaranteed to be Some because index represents the index of
            // the first None in the vec. So it's safe to unwrap
            .map(|e| e.expect("expected only Some elements"))
            .collect();
        self.left += index as u64;

        Some((self.left, elements))
    }

    /// Drops the leading elements in the window. Slides the window to the right,
    /// by num_to_drop, even if it exceeds the capacity of the window
    pub fn drop_front(&mut self, num_to_drop: usize) -> u64 {
        let num_to_drain = std::cmp::min(self.data.len(), num_to_drop);
        self.data.drain(..num_to_drain);
        self.left += num_to_drop as u64;
        self.left
    }

    #[cfg(test)]
    /// Clears all elements in the window and changes the left bounds to a new seqnum.
    fn clear(&mut self, left_bounds: u64) {
        assert_ne!(left_bounds, 0, "Left bounds must be greater than 0");
        self.data.clear();
        self.left = left_bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<T: Debug> BufferWindow<T> {
        // We use this iter instead of IntoIterator until impl's in return position in traits are supported
        // https://rustc-dev-guide.rust-lang.org/return-position-impl-trait-in-trait.html
        // Iter goes to the right_edge of the window, not to the right_bounds
        pub fn iter(&self) -> WindowIter<'_, T> {
            WindowIter {
                seqnum: self.left_bounds(),
                window: self,
            }
        }
    }

    #[derive(Debug)]
    pub struct WindowIter<'a, T: Debug> {
        seqnum: u64,
        window: &'a BufferWindow<T>,
    }

    impl<'a, T: Debug> Iterator for WindowIter<'a, T> {
        type Item = Option<&'a T>;

        fn next(&mut self) -> Option<Self::Item> {
            let right_edge = self.window.max_seen_seqnum();
            if self.seqnum > right_edge {
                None
            } else {
                let result = self.window.get(self.seqnum);
                self.seqnum += 1;
                Some(result)
            }
        }
    }

    #[test]
    fn window_basics() {
        let max_size = 4;
        let mut base = 1000;
        let mut w = BufferWindow::new(max_size, 1);

        assert_eq!(w.capacity(), max_size);

        // initializes to correct bounds
        assert_eq!(w.left_bounds(), 1);
        assert_eq!(w.right_bounds(), 4);

        // Past bounds checking
        assert_eq!(w.put(0, 0), Err(WindowError::BeforeWindow));
        assert_eq!(w.put(5, 0), Err(WindowError::AfterWindow));
        assert_eq!(w.capacity(), max_size);

        // Fill up initial window without changing bounds
        for s in 1..=max_size as u64 {
            assert_eq!(w.put(s, base + s), Ok(()));
            assert_eq!(w.left_bounds(), 1);
            assert_eq!(w.right_bounds(), 4);
            assert_eq!(w.capacity(), max_size);
        }

        assert!(w.is_full());

        // allows overwriting
        base = 2000;
        for s in 1..=max_size as u64 {
            assert_eq!(w.put(s, base + s), Ok(()));
            assert_eq!(w.left_bounds(), 1);
            assert_eq!(w.right_bounds(), 4);
            assert_eq!(w.capacity(), max_size);
        }

        // drains contiguous front and updates bounds
        assert_eq!(w.drain_front(), Some((5, vec![2001, 2002, 2003, 2004])));
        assert_eq!(w.left_bounds(), 5);
        assert_eq!(w.right_bounds(), 8);
        assert_eq!(w.capacity(), max_size);

        // Past bounds checking
        assert_eq!(w.put(1, 0), Err(WindowError::BeforeWindow));
        assert_eq!(w.put(4, 0), Err(WindowError::BeforeWindow));
        assert_eq!(w.put(9, 0), Err(WindowError::AfterWindow));
        assert_eq!(w.put(20, 0), Err(WindowError::AfterWindow));

        // non-contiguous puts
        assert_eq!(w.put(6, 3002), Ok(()));
        assert_eq!(w.put(8, 3004), Ok(()));
        assert!(w.is_full());
        assert_eq!(w.capacity(), max_size);

        // no drain, window does not move
        assert_eq!(w.drain_front(), None);
        assert_eq!(w.left_bounds(), 5);
        assert_eq!(w.right_bounds(), 8);
        assert_eq!(w.capacity(), max_size);

        // partial drain, window moves
        assert_eq!(w.put(5, 3001), Ok(()));
        assert_eq!(w.drain_front(), Some((7, vec![3001, 3002])));
        assert_eq!(w.left_bounds(), 7);
        assert_eq!(w.right_bounds(), 10);
        assert_eq!(w.capacity(), max_size);

        // Past bounds checking
        assert_eq!(w.put(6, 0), Err(WindowError::BeforeWindow));
        assert_eq!(w.put(11, 0), Err(WindowError::AfterWindow));

        // no drain
        assert_eq!(w.drain_front(), None);

        // can drop front, including Nones, advances window
        assert_eq!(w.drop_front(1), 8);
        assert_eq!(w.left_bounds(), 8);
        assert_eq!(w.right_bounds(), 11);
        assert_eq!(w.capacity(), max_size);

        // can now drain, advance window
        assert_eq!(w.drain_front(), Some((9, vec![3004])));
        assert_eq!(w.left_bounds(), 9);
        assert_eq!(w.right_bounds(), 12);
        assert_eq!(w.capacity(), max_size);

        // clear removes all data, and updates window
        assert_eq!(w.put(9, 5001), Ok(()));
        assert_eq!(w.put(10, 5002), Ok(()));
        assert_eq!(w.put(11, 5003), Ok(()));
        assert_eq!(w.put(12, 5004), Ok(()));
        assert_eq!(w.capacity(), max_size);
        w.clear(100);
        assert_eq!(w.capacity(), max_size);
        assert_eq!(w.left_bounds(), 100);
        assert_eq!(w.right_bounds(), 103);
        assert_eq!(w.drain_front(), None);
        assert_eq!(w.put(103, 6001), Ok(()));
        w.clear(200);
        assert_eq!(w.left_bounds(), 200);
        assert_eq!(w.right_bounds(), 203);
        assert_eq!(w.drain_front(), None);

        // drop_front on empty window advances seqnum
        assert_eq!(w.drop_front(9_800), 10_000);
        assert_eq!(w.left_bounds(), 10_000);
        assert_eq!(w.right_bounds(), 10_003);
        assert_eq!(w.capacity(), max_size);

        // drop non-contiguous works
        assert_eq!(w.put(10_003, 4001), Ok(()));
        assert!(w.is_full());
        assert_eq!(w.drop_front(10_000), 20_000);
        assert_eq!(w.left_bounds(), 20_000);
        assert_eq!(w.right_bounds(), 20_003);
    }

    #[test]
    fn window_iter() {
        let mut w: BufferWindow<u64> = BufferWindow::new(4, 1);

        // empty window test
        let mut iter = w.iter();
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);

        // right edge == right bounds
        assert_eq!(w.put(4, 1004), Ok(()));
        let mut iter = w.iter();
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(Some(&1004)));
        assert_eq!(iter.next(), None);

        assert_eq!(w.put(2, 1002), Ok(()));
        let mut iter = w.iter();
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(Some(&1002)));
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(Some(&1004)));
        assert_eq!(iter.next(), None);

        assert_eq!(w.put(1, 1001), Ok(()));
        let mut iter = w.iter();
        assert_eq!(iter.next(), Some(Some(&1001)));
        assert_eq!(iter.next(), Some(Some(&1002)));
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(Some(&1004)));
        assert_eq!(iter.next(), None);

        assert_eq!(w.drain_front(), Some((3, vec![1001, 1002])));
        let mut iter = w.iter();
        assert_eq!(iter.next(), Some(None));
        assert_eq!(iter.next(), Some(Some(&1004)));
        assert_eq!(iter.next(), None);

        assert_eq!(w.drop_front(3), 6);
        let mut iter = w.iter();
        assert_eq!(iter.next(), None);

        assert_eq!(w.put(6, 2001), Ok(()));
        assert_eq!(w.put(7, 2002), Ok(()));
        assert_eq!(w.put(8, 2003), Ok(()));
        let mut iter = w.iter();
        assert_eq!(iter.next(), Some(Some(&2001)));
        assert_eq!(iter.next(), Some(Some(&2002)));
        assert_eq!(iter.next(), Some(Some(&2003)));
        assert_eq!(iter.next(), None);
    }
}

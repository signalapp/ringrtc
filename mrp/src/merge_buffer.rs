//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum AssemblyError {
    ContentLengthExceeded,
}

/// Data structure that buffers data that can be combined with Extend.
/// Buffers in vec so that it makes one allocation on new() and one on merge()
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct MergeBuffer<T> {
    data: Vec<T>,
    content_length: usize,
}

impl<T> MergeBuffer<T>
where
    T: Extend<T>,
{
    /// creates buffer with content_length capacity
    /// returns None if content_length == 0
    pub fn new(content_length: u32) -> Option<Self> {
        if content_length == 0 {
            return None;
        }

        let content_length = content_length as usize;
        Some(Self {
            data: Vec::with_capacity(content_length),
            content_length,
        })
    }

    /// consumes buffer to create combined value
    /// panics if called prematurely
    pub fn merge(self) -> T {
        assert_eq!(self.data.len(), self.content_length);
        let mut iter = self.data.into_iter();
        iter.next()
            .map(|mut first| {
                first.extend(iter);
                first
            })
            .unwrap()
    }

    /// returns AssemblyError::ContentLengthExceeded if content length already reached
    /// returns true if buffer is ready to merge
    pub fn push(&mut self, data: T) -> Result<bool, AssemblyError> {
        if self.data.len() == self.content_length {
            Err(AssemblyError::ContentLengthExceeded)
        } else {
            self.data.push(data);
            Ok(self.data.len() == self.content_length)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::merge_buffer::{AssemblyError, MergeBuffer};

    #[derive(Debug, PartialEq, Eq)]
    struct Extendable(Vec<u32>);

    impl Extend<Extendable> for Extendable {
        fn extend<T: IntoIterator<Item = Extendable>>(&mut self, iter: T) {
            for extendable in iter.into_iter() {
                self.0.extend(extendable.0);
            }
        }
    }

    #[test]
    fn test_merge() {
        let mut buffer = MergeBuffer::new(10).unwrap();
        for i in 1..=10 {
            assert_eq!(Ok(i == 10), buffer.push(Extendable(vec![i])));
        }
        assert_eq!(
            Err(AssemblyError::ContentLengthExceeded),
            buffer.push(Extendable(vec![11]))
        );

        let merged = buffer.merge();
        assert_eq!(Extendable((1..=10).collect::<Vec<_>>()), merged);
    }
}

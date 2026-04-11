//! ZBuf — zero-copy buffer abstraction (zenoh-inspired).
//!
//! A chain of non-contiguous byte slices that avoids copying.
//! Supports scatter-gather I/O patterns common in robotics.

use bytes::Bytes;
use std::collections::VecDeque;

/// Zero-copy buffer composed of multiple byte slices.
///
/// Like zenoh's ZBuf, this allows building messages from multiple
/// non-contiguous memory regions without copying.
#[derive(Clone, Debug, Default)]
pub struct ZBuf {
    slices: VecDeque<Bytes>,
    total_len: usize,
}

impl ZBuf {
    /// Create an empty ZBuf.
    pub fn new() -> Self {
        Self {
            slices: VecDeque::new(),
            total_len: 0,
        }
    }

    /// Create a ZBuf from a single Bytes.
    pub fn from_bytes(data: Bytes) -> Self {
        let len = data.len();
        let mut slices = VecDeque::with_capacity(1);
        slices.push_back(data);
        Self {
            slices,
            total_len: len,
        }
    }

    /// Append a byte slice to the buffer.
    pub fn push(&mut self, data: Bytes) {
        self.total_len += data.len();
        self.slices.push_back(data);
    }

    /// Prepend a byte slice to the buffer.
    pub fn push_front(&mut self, data: Bytes) {
        self.total_len += data.len();
        self.slices.push_front(data);
    }

    /// Total number of bytes across all slices.
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Number of non-contiguous slices.
    pub fn num_slices(&self) -> usize {
        self.slices.len()
    }

    /// Iterate over the byte slices.
    pub fn slices(&self) -> impl Iterator<Item = &Bytes> {
        self.slices.iter()
    }

    /// Collapse all slices into a single contiguous Bytes.
    /// This copies data and should be avoided in hot paths.
    pub fn contiguous(&self) -> Bytes {
        if self.slices.len() == 1 {
            return self.slices[0].clone();
        }

        let mut buf = Vec::with_capacity(self.total_len);
        for slice in &self.slices {
            buf.extend_from_slice(slice);
        }
        Bytes::from(buf)
    }

    /// Read bytes at a given offset without copying.
    /// Returns None if offset + len exceeds buffer bounds.
    pub fn slice(&self, offset: usize, len: usize) -> Option<Bytes> {
        if offset + len > self.total_len {
            return None;
        }

        let mut current_offset = 0;
        let mut result = Vec::with_capacity(len);
        let mut remaining = len;
        let mut skip = offset;

        for slice in &self.slices {
            if skip >= slice.len() {
                skip -= slice.len();
                current_offset += slice.len();
                continue;
            }

            let start = skip;
            let available = slice.len() - start;
            let take = remaining.min(available);

            result.extend_from_slice(&slice[start..start + take]);
            remaining -= take;
            skip = 0;
            current_offset += slice.len();

            if remaining == 0 {
                break;
            }
        }

        let _ = current_offset;
        Some(Bytes::from(result))
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.slices.clear();
        self.total_len = 0;
    }
}

impl From<Bytes> for ZBuf {
    fn from(data: Bytes) -> Self {
        Self::from_bytes(data)
    }
}

impl From<Vec<u8>> for ZBuf {
    fn from(data: Vec<u8>) -> Self {
        Self::from_bytes(Bytes::from(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut buf = ZBuf::new();
        assert!(buf.is_empty());

        buf.push(Bytes::from(vec![1, 2, 3]));
        buf.push(Bytes::from(vec![4, 5, 6]));

        assert_eq!(buf.len(), 6);
        assert_eq!(buf.num_slices(), 2);
    }

    #[test]
    fn test_contiguous() {
        let mut buf = ZBuf::new();
        buf.push(Bytes::from(vec![1, 2]));
        buf.push(Bytes::from(vec![3, 4]));
        buf.push(Bytes::from(vec![5, 6]));

        let data = buf.contiguous();
        assert_eq!(data.as_ref(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_slice_read() {
        let mut buf = ZBuf::new();
        buf.push(Bytes::from(vec![10, 20, 30]));
        buf.push(Bytes::from(vec![40, 50, 60]));

        // Read across slice boundary
        let sub = buf.slice(1, 4).unwrap();
        assert_eq!(sub.as_ref(), &[20, 30, 40, 50]);

        // Out of bounds
        assert!(buf.slice(5, 3).is_none());
    }

    #[test]
    fn test_push_front() {
        let mut buf = ZBuf::new();
        buf.push(Bytes::from(vec![3, 4]));
        buf.push_front(Bytes::from(vec![1, 2]));

        let data = buf.contiguous();
        assert_eq!(data.as_ref(), &[1, 2, 3, 4]);
    }
}

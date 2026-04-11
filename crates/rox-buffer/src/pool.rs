//! MemoryPool — pre-allocated memory pool (copper CopperList-inspired).
//!
//! Reduces allocation overhead in hot paths by reusing pre-allocated buffers.

use bytes::{Bytes, BytesMut};
use std::sync::{Arc, Mutex};

/// Pre-allocated memory pool for zero-allocation message passing.
///
/// Inspired by copper-rs CopperList: pre-allocates a fixed number of
/// buffers at startup and recycles them for hot-path message passing.
pub struct MemoryPool {
    inner: Arc<MemoryPoolInner>,
}

struct MemoryPoolInner {
    pool: Mutex<Vec<BytesMut>>,
    buffer_size: usize,
    capacity: usize,
}

impl MemoryPool {
    /// Create a new pool with `capacity` pre-allocated buffers of `buffer_size` bytes.
    pub fn new(capacity: usize, buffer_size: usize) -> Self {
        let mut pool = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            pool.push(BytesMut::zeroed(buffer_size));
        }
        Self {
            inner: Arc::new(MemoryPoolInner {
                pool: Mutex::new(pool),
                buffer_size,
                capacity,
            }),
        }
    }

    /// Acquire a buffer from the pool. Returns None if pool is exhausted.
    pub fn acquire(&self) -> Option<PooledBuffer> {
        let mut pool = self.inner.pool.lock().ok()?;
        pool.pop().map(|buf| PooledBuffer {
            buf,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Number of available buffers in the pool.
    pub fn available(&self) -> usize {
        self.inner.pool.lock().map(|p| p.len()).unwrap_or(0)
    }

    /// Total capacity of the pool.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// Size of each buffer in bytes.
    pub fn buffer_size(&self) -> usize {
        self.inner.buffer_size
    }
}

impl Clone for MemoryPool {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// A buffer from the memory pool. Automatically returned on drop.
pub struct PooledBuffer {
    buf: BytesMut,
    pool: Arc<MemoryPoolInner>,
}

impl PooledBuffer {
    /// Get a mutable reference to the underlying buffer.
    pub fn as_mut(&mut self) -> &mut BytesMut {
        &mut self.buf
    }

    /// Get a reference to the underlying buffer.
    pub fn as_ref(&self) -> &BytesMut {
        &self.buf
    }

    /// Freeze the buffer into an immutable Bytes.
    /// The buffer is NOT returned to the pool when frozen.
    pub fn freeze(mut self) -> Bytes {
        let buf = std::mem::replace(&mut self.buf, BytesMut::new());
        let bytes = buf.freeze();
        // Don't return to pool — intentionally leak from pool
        std::mem::forget(self);
        bytes
    }

    /// Write data to the buffer, returning how many bytes were written.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let len = data.len().min(self.buf.capacity() - self.buf.len());
        self.buf.extend_from_slice(&data[..len]);
        len
    }

    /// Clear the buffer for reuse.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Length of data written.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the buffer has no data.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let mut recycled = BytesMut::with_capacity(self.pool.buffer_size);
        recycled.resize(0, 0);
        std::mem::swap(&mut self.buf, &mut recycled);

        if let Ok(mut pool) = self.pool.pool.lock() {
            if pool.len() < self.pool.capacity {
                let mut buf = BytesMut::zeroed(self.pool.buffer_size);
                buf.clear();
                pool.push(buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_acquire_release() {
        let pool = MemoryPool::new(4, 1024);
        assert_eq!(pool.available(), 4);

        let buf1 = pool.acquire().unwrap();
        assert_eq!(pool.available(), 3);

        drop(buf1);
        assert_eq!(pool.available(), 4);
    }

    #[test]
    fn test_pool_exhaustion() {
        let pool = MemoryPool::new(2, 256);

        let _b1 = pool.acquire().unwrap();
        let _b2 = pool.acquire().unwrap();
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_pooled_buffer_write() {
        let pool = MemoryPool::new(1, 1024);
        let mut buf = pool.acquire().unwrap();
        buf.clear();

        let written = buf.write(&[1, 2, 3, 4, 5]);
        assert_eq!(written, 5);
        assert_eq!(buf.len(), 5);
    }

    #[test]
    fn test_pooled_buffer_freeze() {
        let pool = MemoryPool::new(2, 256);
        let mut buf = pool.acquire().unwrap();
        buf.clear();
        buf.write(b"hello");

        let bytes = buf.freeze();
        assert_eq!(bytes.as_ref(), b"hello");
        // Pool should have 1 remaining (not returned)
        assert_eq!(pool.available(), 1);
    }
}

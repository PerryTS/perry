//! Stable host memory and explicitly released pool leases.
use std::{cell::RefCell, ptr::NonNull, rc::Rc};

/// A stable, initialized, immutable byte region. Does not own the memory.
#[derive(Debug)]
pub struct IoBuf {
    ptr: NonNull<u8>,
    len: usize,
}
impl IoBuf {
    /// # Safety
    /// `ptr` must be non-null and readable for `len` bytes. Keep the region alive,
    /// unmoved and unmodified until the terminal completion is delivered, or until
    /// the driver is dropped (which must quiesce all native I/O before returning).
    pub unsafe fn from_raw_parts(ptr: *const u8, len: usize) -> Self {
        assert!(
            len <= isize::MAX as usize,
            "buffer length fits a Rust slice"
        );
        Self {
            ptr: NonNull::new(ptr.cast_mut()).expect("non-null buffer pointer"),
            len,
        }
    }
    /// Return the number of bytes in this region.
    pub fn len(&self) -> usize {
        self.len
    }
    /// Whether this region contains zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Return the stable address of the readable byte region.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }
}
/// A stable, exclusively writable region. Does not own the memory.
#[derive(Debug)]
pub struct IoBufMut {
    ptr: NonNull<u8>,
    len: usize,
}
impl IoBufMut {
    /// # Safety
    /// `ptr` must be non-null and writable for `len` bytes. The region must remain
    /// alive and unmoved, and no other access is permitted, until delivery of the
    /// terminal completion or return from driver destruction.
    pub unsafe fn from_raw_parts(ptr: *mut u8, len: usize) -> Self {
        assert!(
            len <= isize::MAX as usize,
            "buffer length fits a Rust slice"
        );
        Self {
            ptr: NonNull::new(ptr).expect("non-null buffer pointer"),
            len,
        }
    }
    /// Return the number of bytes in this region.
    pub fn len(&self) -> usize {
        self.len
    }
    /// Whether this region contains zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Return the stable address of the exclusively writable byte region.
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}
#[derive(Debug)]
/// Choose host-provided memory or an explicitly released pool lease for a read.
pub enum ReadBuf {
    /// Host-provided stable memory, retained under its constructor lifetime contract.
    Provided(IoBufMut),
    /// Use a reusable pool buffer; exhaustion delays progress until a lease is released.
    Pooled,
}
#[derive(Debug)]
/// Stable write data, either borrowed under an unsafe lifetime contract or owned.
pub enum WriteBuf {
    /// Host-provided stable memory, retained under its constructor lifetime contract.
    Provided(IoBuf),
    /// Owned initialized bytes, retained until the operation terminates.
    Owned(Vec<u8>),
}
impl WriteBuf {
    /// Borrow the initialized bytes covered by this buffer.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Provided(b) => {
                // SAFETY: construction promises a readable region for the entire op lifetime.
                unsafe { std::slice::from_raw_parts(b.as_ptr(), b.len()) }
            }
            Self::Owned(b) => b,
        }
    }
}

/// Fixed upper bound keeps writev descriptors inline in the operation table.
pub const MAX_IOV: usize = 8;
#[derive(Debug)]
/// Up to MAX_IOV owned or provided segments stored inline.
pub struct WriteVectored {
    /// Inline segments in transmission order; unused entries are None.
    pub bufs: [Option<WriteBuf>; MAX_IOV],
}
impl WriteVectored {
    /// Build inline vectored storage; returns InvalidInput if more than MAX_IOV segments are supplied.
    pub fn new(bufs: impl IntoIterator<Item = WriteBuf>) -> crate::Result<Self> {
        let mut out = Self {
            bufs: std::array::from_fn(|_| None),
        };
        for (i, b) in bufs.into_iter().enumerate() {
            if i == MAX_IOV {
                return Err(crate::Error::new(crate::ErrorKind::InvalidInput));
            }
            out.bufs[i] = Some(b);
        }
        Ok(out)
    }
}

#[derive(Clone, Debug)]
/// Fixed reusable read storage; leases return buffers when explicitly released or dropped.
pub struct BufferPool {
    inner: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl BufferPool {
    /// Allocate the fixed number and size of reusable read buffers.
    pub fn new(count: usize, size: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new((0..count).map(|_| vec![0; size]).collect())),
        }
    }
    #[cfg(any(
        turnloop_backend = "kqueue",
        turnloop_backend = "epoll",
        turnloop_backend = "iocp",
        turnloop_backend = "wasi_p2",
        all(turnloop_backend = "wasi_p3", feature = "wasi-p3-experimental")
    ))]
    pub(crate) fn available(&self) -> bool {
        !self.inner.borrow().is_empty()
    }
    /// Exhaustion applies backpressure: the backend keeps the read pending.
    pub fn acquire(&self) -> Option<BufLease> {
        self.inner.borrow_mut().pop().map(|data| BufLease {
            data: Some(data),
            len: 0,
            pool: self.inner.clone(),
        })
    }
}
/// Owns the buffer until explicitly dropped/released. May safely survive turns.
/// Keeping all leases applies backpressure; it never allocates a new pool buffer.
#[derive(Debug)]
pub struct BufLease {
    data: Option<Vec<u8>>,
    len: usize,
    pool: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl BufLease {
    /// Borrow the initialized bytes covered by this buffer.
    pub fn as_slice(&self) -> &[u8] {
        &self.data.as_ref().expect("live lease")[..self.len]
    }
    /// Borrow the full exclusive pool-buffer storage for a backend read.
    pub fn writable(&mut self) -> &mut [u8] {
        self.data.as_mut().expect("live lease")
    }
    /// Set the initialized completion length; panics if it exceeds buffer capacity.
    pub fn set_len(&mut self, len: usize) {
        assert!(len <= self.data.as_ref().expect("live lease").len());
        self.len = len;
    }
    /// Return this lease to its originating pool.
    pub fn release(self) {}
}
impl Drop for BufLease {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            self.pool.borrow_mut().push(data);
        }
    }
}

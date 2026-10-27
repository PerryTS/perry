//! Bounded per-watch record storage shared by the native watch backends.
//!
//! Native events are parsed as soon as the OS reports them, so one slow host
//! never stalls another watch sharing a kernel queue. Records wait here until a
//! pooled lease is available. A record that does not fit is dropped and the next
//! batch reports `overflow`, which tells the host to rescan the watched scope.
use crate::{
    BufLease,
    fs::{RECORD_HEADER, WatchKind, put_record},
};

#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
pub(crate) struct Ring {
    bytes: Box<[u8]>,
    len: usize,
    overflow: bool,
}
#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
impl Ring {
    /// Records retained per watch, allocated once when the watch starts.
    pub const CAPACITY: usize = 16 * 1024;
    pub fn new() -> Self {
        Self {
            bytes: vec![0; Self::CAPACITY].into_boxed_slice(),
            len: 0,
            overflow: false,
        }
    }
    /// Storage-free placeholder for watches whose records live elsewhere.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn empty() -> Self {
        Self {
            bytes: Box::default(),
            len: 0,
            overflow: false,
        }
    }
    pub fn push(&mut self, kind: WatchKind, name: &[u8]) {
        if !put_record(&mut self.bytes, &mut self.len, kind as u8, name) {
            self.overflow = true;
        }
    }
    /// Append a record whose UTF-16 name is encoded as WTF-8 in place.
    #[cfg(windows)]
    pub fn push_wide(&mut self, kind: WatchKind, name: &[u16]) {
        let start = self.len;
        let mut end = start + RECORD_HEADER;
        if end > self.bytes.len()
            || !crate::fs::wtf8(name, &mut self.bytes, &mut end)
            || end - start - RECORD_HEADER > usize::from(u16::MAX)
        {
            self.overflow = true;
            return;
        }
        let length = (end - start - RECORD_HEADER) as u16;
        self.bytes[start] = kind as u8;
        self.bytes[start + 1] = 0;
        self.bytes[start + 2..start + 4].copy_from_slice(&length.to_le_bytes());
        self.len = end;
    }
    pub fn lost(&mut self) {
        self.overflow = true;
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0 && !self.overflow
    }
    pub fn clear(&mut self) {
        self.len = 0;
        self.overflow = false;
    }
    /// Move the leading whole records that fit into `lease`, returning (and
    /// clearing) the overflow flag. A record larger than an empty lease is dropped
    /// as lost, so a small pool buffer can never stall delivery.
    pub fn take(&mut self, lease: &mut BufLease) -> bool {
        let out = lease.writable();
        let (mut used, mut at) = (0, 0);
        while at + RECORD_HEADER <= self.len {
            let size = RECORD_HEADER
                + usize::from(u16::from_le_bytes([self.bytes[at + 2], self.bytes[at + 3]]));
            if used + size > out.len() {
                if used != 0 {
                    break;
                }
                self.overflow = true;
            } else {
                out[used..used + size].copy_from_slice(&self.bytes[at..at + size]);
                used += size;
            }
            at += size;
        }
        self.bytes.copy_within(at..self.len, 0);
        self.len -= at;
        lease.set_len(used);
        std::mem::take(&mut self.overflow)
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use crate::{BufferPool, WatchEvents};
    #[test]
    fn batches_preserve_order_and_report_loss_once() {
        let pool = BufferPool::new(1, 12);
        let mut ring = Ring::new();
        ring.push(WatchKind::Rename, b"a");
        ring.push(WatchKind::Change, b"bcdef");
        ring.push(WatchKind::Change, b"this name cannot fit a lease");
        ring.push(WatchKind::Rename, b"z");
        let mut lease = pool.acquire().expect("lease");
        assert!(!ring.take(&mut lease));
        assert_eq!(
            WatchEvents::new(lease.as_slice()).collect::<Vec<_>>(),
            [(WatchKind::Rename, &b"a"[..])]
        );
        drop(lease);
        let mut lease = pool.acquire().expect("lease");
        assert!(!ring.take(&mut lease));
        assert_eq!(lease.as_slice(), b"\x02\x00\x05\x00bcdef");
        drop(lease);
        let mut lease = pool.acquire().expect("lease");
        assert!(ring.take(&mut lease), "oversized record is reported lost");
        assert_eq!(
            WatchEvents::new(lease.as_slice()).collect::<Vec<_>>(),
            [(WatchKind::Rename, &b"z"[..])]
        );
        assert!(ring.is_empty());
        for _ in 0..Ring::CAPACITY {
            ring.push(WatchKind::Change, b"x");
        }
        assert!(!ring.is_empty());
        drop(lease);
        let mut lease = pool.acquire().expect("lease");
        assert!(ring.take(&mut lease), "full storage reports loss");
    }
}

//! Native-only diagnostic evidence; no shape authority or allocator behavior changes.
use super::*;
use std::fmt::Write;
struct Line {
    bytes: [u8; 2048],
    len: usize,
}
impl std::fmt::Write for Line {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if self.len + text.len() > self.bytes.len() {
            return Err(std::fmt::Error);
        }
        self.bytes[self.len..self.len + text.len()].copy_from_slice(text.as_bytes());
        self.len += text.len();
        Ok(())
    }
}
pub(crate) fn snapshot(label: &str, error: u64) {
    let saved_errno = unsafe { *libc::__errno_location() };
    let _scope = crate::snap_census::record_scope();
    let mut out = Line {
        bytes: [0; 2048],
        len: 0,
    };
    let _ = write!(out, "SNAP22_SHAPES {label} error={error} counters=");
    for next in SHAPE_ID_NEXT.iter().chain(EXOTIC_SHAPE_ID_NEXT.iter()) {
        let _ = write!(
            out,
            "{:08x},",
            next.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    let table = &crate::state::state().shapes;
    let slab = table.slab();
    let _ = write!(
        out,
        " semantic={:x} slab={:?}",
        SHAPE_SEMANTIC_NEXT.load(std::sync::atomic::Ordering::Relaxed),
        slab.snap22_counts()
    );
    let mut pcs = [0; crate::error::MAX_CAPTURED_FRAMES];
    let n = crate::error::capture_ips(&mut pcs);
    let _ = writeln!(out, " pcs={:x?}", &pcs[..n.min(12)]);
    unsafe {
        let mut at = 0;
        while at < out.len {
            let n = libc::write(2, out.bytes.as_ptr().add(at).cast(), out.len - at);
            if n > 0 {
                at += n as usize;
            } else if n < 0 && *libc::__errno_location() == libc::EINTR {
                continue;
            } else {
                break;
            }
        }
        *libc::__errno_location() = saved_errno;
    }
}

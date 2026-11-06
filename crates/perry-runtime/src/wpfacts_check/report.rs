//! Failure formatting never runs on the passing path. Raw site metadata belongs
//! to the executable image; the checker borrows its exact bytes and length.
use super::*;
use std::fmt::Write;

#[derive(Clone, Copy)]
pub(super) struct Witness {
    site: u64,
    len: Option<usize>,
    receiver: Option<u64>,
    callee: Option<u64>,
}

impl Witness {
    pub(super) fn legacy(site: i64) -> Self {
        Self {
            site: site as u64,
            len: None,
            receiver: None,
            callee: None,
        }
    }
    pub(super) fn raw(site: i64, len: i64) -> Self {
        Self {
            site: site as u64,
            len: Some(len as usize),
            receiver: None,
            callee: None,
        }
    }
    pub(super) fn is_legacy(self) -> bool {
        self.len.is_none()
    }
    pub(super) fn receiver(mut self, bits: u64) -> Self {
        self.receiver = Some(bits);
        self
    }
    pub(super) fn callee(mut self, bits: u64) -> Self {
        self.callee = Some(bits);
        self
    }
    #[cold]
    #[inline(never)]
    pub(super) fn fail(self, predicate: &str, detail: String) -> ! {
        let Some(len) = self.len else {
            super::violate(self.site, predicate, detail)
        };
        let metadata = unsafe { std::slice::from_raw_parts(self.site as *const u8, len) };
        let mut line = String::from_utf8_lossy(metadata).into_owned();
        line.push_str(",\"predicate\":");
        quote(&mut line, predicate);
        line.push_str(",\"observed_receiver\":");
        if let Some(bits) = self.receiver {
            let addr = (bits & POINTER_MASK) as usize;
            let header = if bits & TAG_MASK == POINTER_TAG
                && unsafe {
                    crate::value::addr_class::try_read_tracked_gc_header_of_type(
                        addr,
                        crate::gc_object_type_pat!(),
                    )
                }
                .is_some()
            {
                Some(unsafe { &*(addr as *const ObjectHeader) })
            } else {
                None
            };
            let (shape, family) = header
                .map(|h| {
                    (
                        Some(crate::object::layout_desc::layout_id_of_word(
                            h.parent_class_id,
                        )),
                        unsafe { crate::object::layout_desc::family_desc_of(h) }.map(|d| d.family),
                    )
                })
                .unwrap_or_default();
            let _ = write!(
                line,
                "{{\"tag\":{},\"shape_id\":{},\"family\":{},\"kind\":{}}}",
                bits >> 48,
                number(shape),
                number(family),
                kind_of(bits)
            );
        } else {
            line.push_str("null");
        }
        line.push_str(",\"observed_member\":");
        quote(&mut line, &detail);
        let _ = write!(
            line,
            ",\"checks_executed\":{}",
            CHECKS.load(std::sync::atomic::Ordering::Relaxed)
        );
        line.push_str(",\"observed_callee\":");
        if let Some(bits) = self.callee {
            let _ = write!(line, "{bits}");
        } else {
            line.push_str("null");
        }
        line.push_str("}\n");
        unsafe {
            let mut rest = line.as_bytes();
            while !rest.is_empty() {
                let n = libc::write(2, rest.as_ptr().cast(), rest.len());
                if n > 0 {
                    rest = &rest[n as usize..];
                } else if n < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                } else {
                    break;
                }
            }
            libc::_exit(86)
        }
    }
}

fn number(v: Option<u32>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
}

fn quote(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

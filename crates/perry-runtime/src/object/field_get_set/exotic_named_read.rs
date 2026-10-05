//! Shape-gated exotic reads. Outlined to keep probe work inside the gate.
use super::*;

/// A local dispatch answer derived only from the existing header and shape.
/// Native is zero: its first branch preserves the original inline typed
/// read. Non-object headers retain the original exotic probe sequence.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum NamedReadKind {
    Native = 0,
    Other,
    Ordinary,
    Class,
    Object,
}

/// Reuse the live-object classification the class-constructor arm already
/// performs. GC_TYPE_OBJECT rules out typed arrays, Date and Temporal even
/// when its shape is unmarked; only the ordinary shape kinds also rule out
/// URLSearchParams. Unknown/headerless receivers keep the original probes.
#[inline]
pub(super) fn named_read_receiver_kind(obj: *const ObjectHeader) -> NamedReadKind {
    use super::super::shapes::ShapeObjectKind;
    let bits = obj as u64;
    let addr = match bits >> 48 {
        0 => bits as usize,
        0x7FFD => (bits & crate::value::POINTER_MASK) as usize,
        _ => return NamedReadKind::Other,
    };
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return NamedReadKind::Other;
        };
        match header.obj_type {
            crate::gc::GC_TYPE_OBJECT => {
                let kind = if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
                    super::super::shapes::shape_object_kind_by_id(
                        (*(addr as *const ObjectHeader)).parent_class_id,
                    )
                } else {
                    None
                };
                match kind {
                    Some(ShapeObjectKind::Class) if bits >> 48 == 0 => NamedReadKind::Class,
                    Some(ShapeObjectKind::Ordinary | ShapeObjectKind::OrdinaryNumericProof) => {
                        NamedReadKind::Ordinary
                    }
                    _ => NamedReadKind::Object,
                }
            }
            _ => NamedReadKind::Native,
        }
    }
}

// An explicit C-layout pair lets an outlined read return its verdict and
// value in registers. Rust's Option<JSValue> ABI uses an indirect result and
// adds stores on every exotic read. The inline wrappers keep the usual
// Option spelling at call sites, without a new JSValue sentinel.
#[repr(C)]
struct ProbeRead {
    value: JSValue,
    matched: u64,
}
impl ProbeRead {
    #[inline]
    fn hit(value: JSValue) -> Self {
        Self { value, matched: 1 }
    }
    #[inline]
    fn miss() -> Self {
        Self {
            value: JSValue::undefined(),
            matched: 0,
        }
    }
    #[inline]
    fn into_option(self) -> Option<JSValue> {
        if self.matched != 0 {
            Some(self.value)
        } else {
            None
        }
    }
}

#[inline]
pub(super) unsafe fn search_params_read(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    key_bytes: &[u8],
) -> Option<JSValue> {
    search_params_read_impl(obj, key, key_bytes.as_ptr(), key_bytes.len()).into_option()
}

// Outlining keeps Cell and URL probe work inside their kind guards.

#[cfg(test)]
thread_local! {
    // Total counts retain the exotic positive controls. Separate shape-kind
    // counts distinguish the original receiver from recursive builtin-host
    // reads; Object.prototype is intentionally OrdinaryUnmarked.
    static ORDINARY_PROBES: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) };
    static PROBES: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) };
}

#[cfg(test)]
pub(super) fn note_probe(index: usize, obj: *const ObjectHeader) {
    PROBES.with(|probes| {
        let mut counts = probes.get();
        counts[index] += 1;
        probes.set(counts);
    });
    let bits = obj as u64;
    let addr = match bits >> 48 {
        0 => bits as usize,
        0x7FFD => (bits & crate::value::POINTER_MASK) as usize,
        _ => return,
    };
    // Observe the immutable descriptor directly, independently of the gate
    // under test, so replacing that gate with false still trips the assertion.
    let ordinary = unsafe {
        crate::value::addr_class::try_read_gc_header(addr).is_some_and(|header| {
            header.obj_type == crate::gc::GC_TYPE_OBJECT
                && super::super::shapes::object_shape_descriptor(addr as *const ObjectHeader)
                    .is_some_and(|shape| {
                        matches!(
                            shape.object_kind,
                            super::super::shapes::ShapeObjectKind::Ordinary
                                | super::super::shapes::ShapeObjectKind::OrdinaryNumericProof
                        )
                    })
        })
    };
    if ordinary {
        ORDINARY_PROBES.with(|probes| {
            let mut counts = probes.get();
            counts[index] += 1;
            probes.set(counts);
        });
    }
}

#[cfg(test)]
pub(super) fn ordinary_probe_counts() -> [u64; 3] {
    ORDINARY_PROBES.with(|probes| probes.get())
}

#[cfg(test)]
pub(super) fn probe_counts() -> [u64; 3] {
    PROBES.with(|probes| probes.get())
}

#[inline(never)]
unsafe extern "C-unwind" fn search_params_read_impl(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    key_bytes_ptr: *const u8,
    key_bytes_len: usize,
) -> ProbeRead {
    let key_bytes = std::slice::from_raw_parts(key_bytes_ptr, key_bytes_len);
    #[cfg(test)]
    note_probe(2, obj);
    // #5961: native URLSearchParams is an ordinary object (class_id == 0,
    // leading `_entries` slot) whose method surface normally exists only
    // via static type-directed lowering. A type-erased receiver lands
    // here instead — resolve the methods dynamically so `sp.append(...)`
    // stays callable, and `size` reads as a number.
    if !key.is_null() && crate::url::search_params::shape_is_url_search_params(obj) {
        if let Ok(name) = std::str::from_utf8(key_bytes) {
            if name == "size" {
                let n =
                    crate::url::search_params::js_url_search_params_size(obj as *mut ObjectHeader);
                return ProbeRead::hit(JSValue::from_bits((n as f64).to_bits()));
            }
            if let Some(v) = crate::url::search_params::url_search_params_method_value(obj, name) {
                return ProbeRead::hit(JSValue::from_bits(v.to_bits()));
            }
        }
    }

    ProbeRead::miss()
}

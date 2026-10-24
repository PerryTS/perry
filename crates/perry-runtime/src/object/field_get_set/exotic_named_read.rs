//! Shape-gated exotic reads: the receiver classification that keeps the
//! native-cell probes off ordinary objects.
use super::*;

/// A local dispatch answer derived only from the existing header and shape.
/// Native is zero: its first branch preserves the original inline typed
/// read. Non-object headers retain the original exotic probe sequence.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum NamedReadKind {
    Native = 0,
    Other,
    Class,
    Object,
}

/// Reuse the live-object classification the class-constructor arm already
/// performs. GC_TYPE_OBJECT rules out typed arrays, Date and Temporal even
/// when its shape is unmarked. Unknown/headerless receivers keep the original
/// probes.
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
                    super::super::shapes::shape_object_kind_of_receiver(
                        (*(addr as *const ObjectHeader)).shape_word(),
                    )
                } else {
                    None
                };
                match kind {
                    Some(ShapeObjectKind::Class) if bits >> 48 == 0 => NamedReadKind::Class,
                    _ => NamedReadKind::Object,
                }
            }
            _ => NamedReadKind::Native,
        }
    }
}

#[cfg(test)]
thread_local! {
    // Total counts retain the exotic positive controls. Separate shape-kind
    // counts distinguish the original receiver from recursive builtin-host
    // reads; Object.prototype is intentionally OrdinaryUnmarked.
    static ORDINARY_PROBES: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([0; 2]) };
    static PROBES: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([0; 2]) };
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
pub(super) fn ordinary_probe_counts() -> [u64; 2] {
    ORDINARY_PROBES.with(|probes| probes.get())
}

#[cfg(test)]
pub(super) fn probe_counts() -> [u64; 2] {
    PROBES.with(|probes| probes.get())
}

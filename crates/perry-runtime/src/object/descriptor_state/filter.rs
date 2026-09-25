//! The per-owner filters a descriptor lookup runs before it touches a table:
//! one header read picks the route (an ordinary object's keys, a cell's meta
//! summary, or the tables), split out of `descriptor_state.rs` for the
//! 2000-line cap.

use super::*;

/// Where a descriptor lookup for `owner` is answered, from ONE header read.
#[derive(Clone, Copy)]
pub(super) enum DescriptorRoute {
    /// An ordinary object: its keys carry every key's attributes (charter step 3).
    Keys,
    /// A cell with a meta edge: its summary words filter the tables (null = none).
    Meta(*mut ObjectMeta),
    /// Anything else: probe the tables.
    Tables,
}

#[inline]
pub(super) unsafe fn descriptor_route(owner: usize) -> DescriptorRoute {
    let Some(header) = crate::value::addr_class::try_read_gc_header(owner) else {
        return DescriptorRoute::Tables;
    };
    if header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && crate::typedarray::lookup_typed_array_kind(owner).is_none()
    {
        return DescriptorRoute::Keys;
    }
    match super::cell_meta_slot_for_header(owner, header) {
        Some(slot) => DescriptorRoute::Meta(*slot),
        None => DescriptorRoute::Tables,
    }
}

#[inline]
pub(super) unsafe fn meta_may_have(meta: *mut ObjectMeta, key: &str, accessor: bool) -> bool {
    if meta.is_null() {
        return false;
    }
    let word = if accessor {
        (*meta).accessor_key_bits
    } else {
        (*meta).attr_key_bits
    };
    word & descriptor_key_bit(key) != 0
}

/// #6759 Phase C2 per-key fast-path verdict: can the string-keyed
/// descriptor tables hold an entry `(owner, key)`? `false` is
/// authoritative (the probe is skipped); `true` means "probe the table"
/// (a genuine entry, a Bloom collision, or a non-meta-capable owner).
#[inline]
pub(crate) fn may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    unsafe {
        let answer = match descriptor_route(owner) {
            // Charter step 3: exact for an ordinary object — its keys record
            // both halves of every key's descriptor.
            DescriptorRoute::Keys => {
                let owner = owner as *const ObjectHeader;
                return if accessor {
                    super::key_attrs::object_key_is_accessor(owner, key.as_bytes())
                } else {
                    super::key_attrs::object_key_entry(owner, key.as_bytes()) != 0
                };
            }
            DescriptorRoute::Meta(meta) => meta_may_have(meta, key, accessor),
            DescriptorRoute::Tables => true,
        };
        // Diagnostic only, and only when the instrument is armed: one relaxed
        // load otherwise. Counts the RegExp receivers this filter sees and how
        // many it now proves absent — before the meta edge was wired for
        // RegExp the second number was 0 by construction.
        if crate::hot_diag::regex_on() {
            note_regexp_descriptor_probe(owner, answer);
        }
        answer
    }
}

/// Test-only view of [`may_have_descriptor_entry`], so a test can assert the
/// FILTER's answer rather than only the value it filters to. Without this a
/// test can see that `get_property_attrs` returns `None`, which is equally
/// true when the fast negative never fired — it would pass against a change
/// that did nothing.
#[cfg(test)]
pub(crate) fn test_may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    may_have_descriptor_entry(owner, key, accessor)
}

/// Diagnostic counter for [`may_have_descriptor_entry`]: is this owner a
/// RegExp cell, and did the summary prove the key absent? Split out and marked
/// cold so the armed check costs the hot path a predictable branch and nothing
/// else.
#[cold]
unsafe fn note_regexp_descriptor_probe(owner: usize, answer: bool) {
    let Some(header) = crate::value::addr_class::try_read_gc_header(owner) else {
        return;
    };
    if header.obj_type != crate::gc::GC_TYPE_REGEXP {
        return;
    }
    crate::hot_diag::regex_with(|d| {
        d.desc_regexp_probes += 1;
        if !answer {
            d.desc_regexp_meta_negative += 1;
        }
    });
}

//! Runtime reads: a builtin's spec `Get(O, key)` on the one path.
//!
//! A builtin that reads a fixed key off a receiver it was handed (the promise
//! resolution's `Get(resolution, "then")`, the RegExp builtins' `Get(R,
//! "exec")` and flag reads, a stream's state fields) reads it exactly as an
//! emitted `o.key` site whose receiver no compile-time fact names: the
//! unknown-receiver probe (`layout_probe::perry_probe_get`) with the agent's
//! directory, and, when the probe declines, the cacheless complete read
//! (`js_object_get_field_slow`). Nothing is remembered between reads: every
//! word the probe reads is the receiver's own data or immutable layout data,
//! so there is nothing to prime, validate or root.
//!
//! # The key
//!
//! The probe compares key words: a property atom (one per text per agent) or
//! a symbol. A read looks its key's atom up per call (no allocation); a key
//! whose text has no atom yet cannot be a key of any list the probe answers
//! from, so the observable read mints it (an allocation, with the receiver
//! rooted) and the non-observable [`probe`] declines.
use crate::object::ObjectHeader;

/// A fixed key of a runtime read: a name or a symbol (its address).
#[derive(Clone, Copy)]
pub(crate) enum Key<'a> {
    Name(&'a [u8]),
    Symbol(usize),
}

/// What `Get(O, key)` would do, found without running anything.
#[derive(Clone, Copy)]
pub(crate) enum Answer {
    /// It returns this value (an own or inherited data property, or
    /// `undefined` for a proven absence).
    Data(u64),
    /// It calls this getter (NaN-boxed closure bits).
    Getter(u64),
}

/// The probe's key word and hash for `key`, when one exists without
/// allocating.
#[inline]
unsafe fn key_word(key: Key<'_>) -> Option<(u64, u64)> {
    match key {
        Key::Name(name) => {
            let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
            let atom = crate::string::atom_lookup(name, hash)?;
            if !crate::string::is_property_atom(atom) {
                return None;
            }
            Some((
                crate::value::STRING_TAG | atom as u64,
                crate::string::property_atom_hash(atom),
            ))
        }
        Key::Symbol(symbol) => {
            if symbol == 0 {
                return None;
            }
            let bits = crate::value::POINTER_TAG | symbol as u64;
            if crate::symbol::js_is_symbol(f64::from_bits(bits)) == 0 {
                return None;
            }
            Some((bits, (*(symbol as *const crate::symbol::SymbolHeader)).id))
        }
    }
}

/// The probe's answer for `key_bits` on `obj`, or `TAG_HOLE`.
#[inline(always)]
unsafe fn probe_word(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> f64 {
    crate::object::layout_probe::perry_probe_get(
        crate::object::shapes::ordinary_dir_addr(),
        (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key_bits,
        hash,
    )
}

/// `Get(obj, key)` without running anything, when the receiver's layouts
/// answer it: a data value, or the getter it would call. `None` when only
/// the complete (observable) read can answer.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` above the handle band (see
/// [`object_receiver`]).
pub(crate) unsafe fn probe(obj: *const ObjectHeader, key: Key<'_>) -> Option<Answer> {
    let Some((key_bits, hash)) = key_word(key) else {
        return crate::object::method_site::read_holder::shape_answer(obj, key);
    };
    let v = probe_word(obj, key_bits, hash);
    if v.to_bits() != crate::value::TAG_HOLE {
        return Some(Answer::Data(v.to_bits()));
    }
    if let Some(acc) = crate::object::layout_probe::probe_accessor_word(obj, key_bits, hash) {
        // A pair this word cannot read (`Accessor::default()`), a compiled
        // class getter or a setter-only pair: the observable read decides.
        if acc.raw_get != 0 || acc.static_get != 0 || acc.get == 0 {
            return None;
        }
        return Some(Answer::Getter(acc.get));
    }
    // A receiver or holder the probe declines (a builtin prototype): the
    // shapes' walk by key position.
    crate::object::method_site::read_holder::shape_answer(obj, key)
}

/// `Get(obj, key)`: the probe, else the complete read. It can run a getter,
/// collect and throw.
///
/// # Safety
/// As [`probe`]; `key` is the read's fixed key text.
pub(crate) unsafe fn get(obj: *mut ObjectHeader, key: &[u8]) -> f64 {
    let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
    let (atom, obj) = match crate::string::atom_lookup(key, hash) {
        Some(atom) if crate::string::is_property_atom(atom) => (atom, obj),
        _ => {
            // The first read per agent mints the atom: an allocation, so
            // the receiver is rooted across it.
            let scope = crate::gc::RuntimeHandleScope::new();
            let handle = scope.root_raw_mut_ptr(obj);
            handle.across_mut::<ObjectHeader, _>(|| {
                crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0)
                    as *const crate::StringHeader
            })
        }
    };
    let v = probe_word(
        obj,
        crate::value::STRING_TAG | atom as u64,
        crate::string::property_atom_hash(atom),
    );
    if v.to_bits() != crate::value::TAG_HOLE {
        return v;
    }
    super::js_object_get_field_slow(obj as i64, atom)
}

/// The ordinary-object receiver a read reads from: `value` is a pointer
/// above the handle band whose validated GC header says `GC_TYPE_OBJECT`.
/// Every other value (primitives, proxies and handles, arrays, closures,
/// promises, exotic cells) keeps its caller's generic `Get`.
#[inline]
pub(crate) fn object_receiver(value: f64) -> Option<*mut ObjectHeader> {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    (header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some(addr as *mut ObjectHeader)
}

#[cfg(test)]
#[path = "runtime_read_site_tests.rs"]
mod tests;

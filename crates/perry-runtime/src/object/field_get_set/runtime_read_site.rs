//! Runtime reads: a builtin's spec `Get(O, key)` on the one path.
//!
//! A builtin that reads a fixed key off a receiver it was handed (the promise
//! resolution's `Get(resolution, "then")`, the RegExp builtins' `Get(R,
//! "exec")` and flag reads, a stream's state fields) reads it exactly as an
//! emitted `o.key` site whose receiver no compile-time fact names: the
//! unknown-receiver probe (`layout_probe::perry_probe_get`) with the agent's
//! directory, and, when the probe declines, the cacheless complete read
//! (`js_object_get_field_slow`). Fixed-name Rust sites use `get_with_memo`:
//! the same leaf and collecting entrance as an emitted fixed-name read that
//! owns a shape/slot memo (`perry_probe_get_memo`, which asks the memo only
//! after an own miss, then `js_object_get_field_site_slow`). The memo retains
//! no heap addresses or getter values.
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
    get_with_memo(obj, key, std::ptr::null_mut())
}

/// [`get`] for a fixed-key Rust site that owns a shape/slot memo, read as an
/// emitted site that owns one: an own hit never reads the memo; an own miss
/// asks it; only the collecting entrance derives it.
///
/// # Safety
/// As [`get`]; `memo` is null or this site's shared atomic storage.
pub(crate) unsafe fn get_with_memo(
    obj: *mut ObjectHeader,
    key: &[u8],
    memo: *mut crate::object::layout_probe::site_memo::ReadSiteMemo,
) -> f64 {
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
    let key_bits = crate::value::STRING_TAG | atom as u64;
    let hash = crate::string::property_atom_hash(atom);
    let v = if memo.is_null() {
        probe_word(obj, key_bits, hash)
    } else {
        crate::object::layout_probe::site_memo::perry_probe_get_memo(
            crate::object::shapes::ordinary_dir_addr(),
            (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
            key_bits,
            memo,
        )
    };
    if v.to_bits() != crate::value::TAG_HOLE {
        return v;
    }
    if memo.is_null() {
        super::js_object_get_field_slow(obj as i64, atom)
    } else {
        super::js_object_get_field_site_slow(obj as i64, atom, memo)
    }
}

/// The integer a Number property key names, when its canonical text is a
/// plain decimal integer: an INT32-tagged key, or an integral double of
/// magnitude at most 2^53 (`-0` names `"0"`). Every other Number keeps the
/// complete `ToPropertyKey` path.
#[inline]
pub(crate) fn integer_key(key: f64) -> Option<i64> {
    let bits = key.to_bits();
    if bits & crate::value::TAG_MASK == crate::value::INT32_TAG {
        return Some(i64::from(crate::value::JSValue::from_bits(bits).as_int32()));
    }
    (key.is_finite() && key.fract() == 0.0 && key.abs() <= 9_007_199_254_740_992.0)
        .then_some(key as i64)
}

/// `ToString(n)` of an integer of magnitude at most 2^53, written into `buf`.
fn integer_key_text(n: i64, buf: &mut [u8; 24]) -> &[u8] {
    let mut i = buf.len();
    let mut m = n.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if n < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    &buf[i..]
}

/// `Get(obj, n)` for a computed Number key `n` (see [`integer_key`]) by the
/// receiver's layouts: `arr[arr.length - 1]` on an empty array (`"-1"`),
/// `record[id]` on an ordinary object. The key's text is formed on the stack
/// and read by its atom exactly as an emitted `obj["-1"]` / `obj["7"]` site
/// reads it: an array-index text may be an element key, which no layout
/// lists, so it takes only an answer the probe finds
/// (`perry_probe_get_binding`), never absence; any other text takes the
/// probe's whole answer, absence included. A non-index text with no atom yet
/// mints it (the receiver rooted across the allocation), as [`get`] does; an
/// index text with no atom is in no list, so the complete read answers.
///
/// `Ok(value)`, or `Err(receiver)`, the receiver's current address, when only
/// the complete read can answer.
///
/// # Safety
/// `obj` is a live heap object above the handle band (an ordinary object or
/// an Array); the probe classifies it from its own header.
pub(crate) unsafe fn probe_integer_key(
    obj: *mut ObjectHeader,
    n: i64,
) -> Result<f64, *mut ObjectHeader> {
    let mut buf = [0u8; 24];
    let text = integer_key_text(n, &mut buf);
    let index = (0..i64::from(u32::MAX)).contains(&n);
    // An index key of an object with an elements store (an Array subclass
    // instance, an object that took index stores) lives in that store, which
    // the probe does not read: the complete read answers. Only an ordinary
    // object is handed an index key (an Array's index is an element read).
    if index {
        // A1_UNPROVEN: record-first: runtime read site (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        let class_id = (*obj).class_id;
        if (!meta.is_null() && (*meta).elements != 0)
            || (class_id != 0 && crate::array::is_array_subclass_class_id(class_id))
        {
            return Err(obj);
        }
    }
    let hash = crate::object::key_bytes_hash(text.as_ptr(), text.len());
    let (atom, obj) = match crate::string::atom_lookup(text, hash) {
        Some(atom) if crate::string::is_property_atom(atom) => (atom, obj),
        _ if index => return Err(obj),
        _ => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let handle = scope.root_raw_mut_ptr(obj);
            let atom = crate::string::js_string_pool_atom(text.as_ptr(), text.len() as u32, hash, 0)
                as *const crate::StringHeader;
            (atom, handle.get_raw_mut_ptr::<ObjectHeader>())
        }
    };
    let key_bits = crate::value::STRING_TAG | atom as u64;
    let key_hash = crate::string::property_atom_hash(atom);
    let dir = crate::object::shapes::ordinary_dir_addr();
    let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
    let v = if index {
        crate::object::layout_probe::perry_probe_get_binding(dir, biased, key_bits, key_hash)
    } else {
        crate::object::layout_probe::perry_probe_get(dir, biased, key_bits, key_hash)
    };
    if v.to_bits() != crate::value::TAG_HOLE {
        Ok(v)
    } else {
        Err(obj)
    }
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
    (crate::gc_type_is_object!(header.obj_type)
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some(addr as *mut ObjectHeader)
}

#[cfg(test)]
#[path = "runtime_read_site_tests.rs"]
mod tests;

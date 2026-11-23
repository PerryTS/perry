//! The own-key stores a probe reads past an ordinary layout: an array's or a
//! function's named bag, the exotic receivers, and the overflow fallback.

use super::*;

/// The own-key store [`bag_own`]'s one tail reads, once the
/// receiver kind named it: an array's named bag, or a function's store as
/// `closure::props::own_store_in` hands it over (also to [`exotic_get`]).
/// Only this module opens it.
#[derive(Clone, Copy)]
pub(crate) struct OwnStore(pub(super) *const ObjectHeader);

impl OwnStore {
    /// # Safety
    /// `store` is null (no store) or a live ordinary store object.
    #[inline(always)]
    pub(crate) unsafe fn new(store: *const ObjectHeader) -> Self {
        Self(store)
    }
}

/// The own key `key_bits` (hash `hash`) of function `closure`, read from its
/// bag by the ordinary own arm ([`bag_own`]), the arm an exotic receiver's
/// read takes first. A bag record whose probe word is not derived yet is
/// derived here, as [`probe_word`] does for any ordinary read.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline(never)]
pub(crate) unsafe fn closure_bag_own(
    closure: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Result<f64, bool> {
    crate::closure::props::own_lookup(closure as usize, key_bits, hash)
}

/// Do the own keys of function `closure` provably lack `call`, `apply` and
/// `bind`? Answered by structure, never by a lookup: a function without a
/// store owns only its family's born keys, whose record carries the
/// publication-time absence fact; a function with a store owns its store
/// record's keys, whose record carries the same fact. `false` = not proven
/// (the caller asks the store).
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn closure_own_keys_lack_function_methods(
    closure: *const crate::closure::ClosureHeader,
) -> bool {
    let header = crate::gc::header_from_trusted_user_ptr(closure.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return false;
    }
    let bag = crate::closure::props::bag_of(closure as usize) as *const ObjectHeader;
    if bag.is_null() {
        if !crate::closure::shape::closure_on_plain_function_shape(closure) {
            // No store and no family keys: nothing is own (`bagless_own`).
            return true;
        }
        let r = family_record_of(super::super::ordinary_dir_addr(), (*closure).shape_id);
        return !r.is_null() && (*r).proves_no_function_method_keys();
    }
    let word = (*bag).parent_class_id;
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        return false;
    }
    let r = record_of(super::super::ordinary_dir_addr(), LayoutWord(word));
    !r.is_null() && (*r).proves_no_function_method_keys()
}

/// A bag-less plain function's own key: its family record holds its own keys
/// (`length`, `name`; a constructor's `prototype` is not in it yet, and
/// nothing materialized it without a store). `Err(true)` for any other closure.
#[inline(always)]
pub(crate) unsafe fn bagless_own(
    closure: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Result<f64, bool> {
    if crate::closure::shape::closure_on_plain_function_shape(closure) {
        let r = family_record_of(super::super::ordinary_dir_addr(), (*closure).shape_id);
        // No store holds nothing: a miss in the record is a miss in the store.
        return match family_position(r, key_bits, hash) {
            Ok((pos, load)) => Ok(family_hit(closure, r, pos, load)),
            Err(_) => Err(true),
        };
    }
    Err(true)
}

/// The own key of the ordinary store object `bag` (non-null), read through
/// its record in the ordinary directory, deriving a probe word not derived
/// yet, as [`probe_word`] does for any ordinary read. `None` when the
/// record cannot answer (shadowed, no record, probe not ready); `Some` the
/// record's answer.
#[inline(always)]
pub(crate) unsafe fn store_own_derived(
    bag: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<Result<f64, bool>> {
    let word = (*bag).parent_class_id;
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        return None;
    }
    let r = record_of(super::super::ordinary_dir_addr(), LayoutWord(word));
    if r.is_null() {
        return None;
    }
    let pw = probe_word(r);
    if pw & (PROBE_READY | PROBE_DECLINE | PROBE_CHECKED | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS
    {
        return None;
    }
    Some(list_position(r, pw, key_bits, hash).map(|(pos, load)| hit_value(bag, r, pos, load)))
}

#[inline(always)]
pub(super) unsafe fn exotic_get<const ABSENT: bool, const BAG: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return hole();
    }
    let (named, proto, function) = match (*header).obj_type {
        crate::gc_array_type_pat!() => {
            // Array length is an intrinsic own property, stored in the array
            // header rather than its named bag. Never prove it absent by
            // walking the bag and prototypes (including for computed keys
            // read by a Proxy's get trap).
            if hash == crate::object::runtime_layouts::atom_hash(b"length") && {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                crate::string::js_string_key_bytes(
                    crate::JSValue::from_bits(key_bits),
                    &mut scratch,
                ) == Some(b"length".as_slice())
            } {
                return (*(obj as *const crate::array::ArrayHeader)).length as f64;
            }
            let named = (*(obj as *const crate::array::ArrayHeader)).named as *const ObjectHeader;
            if (*header)._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
                (named, None, false)
            } else {
                (
                    named,
                    Some(crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE),
                    false,
                )
            }
        }
        crate::gc::GC_TYPE_CLOSURE => {
            let c = obj as *const crate::closure::ClosureHeader;
            let plain = crate::closure::shape::closure_on_plain_function_shape(c);
            // The function's own store, read first (`BAG`) by [`exotic_rest`]
            // as it reads an array's bag.
            let named = crate::closure::props::own_store_in(c as usize).0;
            if plain && named.is_null() {
                // A plain function without a store: its own keys are its
                // family record's ([`bagless_function_get`]).
                return bagless_function_get::<ABSENT>(dir, c, key_bits, hash);
            }
            // A plain or bound function's [[Prototype]] is its shape's
            // identity (`%Function.prototype%` until a link moves it).
            // A bound function's intrinsic `name` and `length` are its
            // target's, not static data of a body: those keys decline.
            let proto = if plain {
                crate::object::shapes::shape_proto_id((*c).shape_id)
            } else if dictionary_function_reads_through(c, hash) {
                Some(crate::closure::shape::INTRINSIC_SERIAL_FUNCTION)
            } else {
                None
            };
            // A plain function with a bag asks its intrinsic keys after the
            // bag.
            (named, proto, plain)
        }
        _ => return hole(),
    };
    exotic_rest::<ABSENT, BAG>(dir, obj, named, proto, function, key_bits, hash)
}

/// A bag-less plain function's read: its family record is its first and
/// only own check and the slab row holding its [[Prototype]] identity
/// ([`family_own`]: a miss for any key but `length`, `name` and `prototype`
/// is one hash test), then the chain from that prototype.
#[inline(never)]
unsafe fn bagless_function_get<const ABSENT: bool>(
    dir: *const u8,
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let family = family_record_of(dir, (*c).shape_id);
    if let Some(v) = family_own(family, c, key_bits, hash) {
        return v;
    }
    let proto = if !family.is_null() {
        Some((*family).proto_id)
    } else {
        crate::object::shapes::shape_proto_id((*c).shape_id)
    };
    exotic_rest::<ABSENT, false>(
        dir,
        c.cast(),
        std::ptr::null(),
        proto,
        false,
        key_bits,
        hash,
    )
}

/// [`exotic_get`] after the receiver's kind gave its bag `named`, its
/// [[Prototype]] identity `proto` and whether it is a plain `function` with
/// a bag: the bag's key (`BAG`), the function's intrinsic keys, the chain.
#[inline(always)]
unsafe fn exotic_rest<const ABSENT: bool, const BAG: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    named: *const ObjectHeader,
    proto: Option<u64>,
    function: bool,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if BAG && !named.is_null() {
        let word = (*named).parent_class_id;
        let r = record_of(dir, LayoutWord(word));
        if r.is_null() {
            return hole();
        }
        match own_position(r, key_bits, hash) {
            Ok(pos) => return value_at(named, r, pos),
            Err(true) if word & crate::object::overflow::SHADOW_BIT != 0 => {
                match overflow_get(named, key_bits, hash) {
                    Some(v) => return v,
                    None => {}
                }
            }
            Err(true) => {}
            Err(false) => return hole(),
        }
    }
    // A bound function is asked only after its bag missed (an own key is
    // the common read).
    let proto = match proto {
        None if (*header).obj_type == crate::gc::GC_TYPE_CLOSURE
            && bound_on_described_shape(obj.cast(), hash) =>
        {
            crate::object::shapes::shape_proto_id(
                (*obj.cast::<crate::closure::ClosureHeader>()).shape_id,
            )
        }
        proto => proto,
    };
    let Some(serial) = proto else {
        return hole();
    };
    if function {
        // Not in the bag: the function layout's own keys.
        if let Some(v) = function_own_intrinsic(obj.cast(), key_bits, hash) {
            return v;
        }
    }
    let Some(p) = intrinsic_prototype(dir, serial) else {
        return hole();
    };
    if serial == crate::closure::shape::INTRINSIC_SERIAL_ARRAY_PROTOTYPE {
        // `p` is `%Array.prototype%`'s holder; `%Array.prototype%` itself
        // (whose holder `named` is) continues at its own [[Prototype]]. A
        // write to the holder, or a relink of `%Array.prototype%`, sets the
        // holder's shadow bit.
        if p != named {
            let w = (*p).parent_class_id;
            let rn = record_of(dir, LayoutWord(w));
            if rn.is_null() || w & crate::object::overflow::SHADOW_BIT != 0 {
                return hole();
            }
            let pw = probe_word(rn);
            if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
                return hole();
            }
            match list_position(rn, pw, key_bits, hash) {
                Ok((pos, load)) => return hit_value(p, rn, pos, load),
                Err(true) => {}
                Err(false) => return hole(),
            }
        }
        // `%Array.prototype%`'s [[Prototype]] is `%Object.prototype%`.
        return match object_prototype(dir) {
            Some(op) => exotic_chain::<ABSENT>(dir, op, key_bits, hash),
            None => hole(),
        };
    }
    exotic_chain::<ABSENT>(dir, p, key_bits, hash)
}

/// Is `c` a bound function on a described Function shape (its own keys are
/// its bag's, its [[Prototype]] its shape's identity), read for a key other
/// than the intrinsic `name`, `length` and `prototype`, which a bound
/// function answers from its target or not at all?
///
/// # Safety
/// `c` is a live closure cell.
#[inline(never)]
unsafe fn bound_on_described_shape(c: *const crate::closure::ClosureHeader, hash: u64) -> bool {
    use crate::object::runtime_layouts::atom_hash as h;
    const NAME: u64 = h(b"name");
    const LENGTH: u64 = h(b"length");
    const PROTOTYPE: u64 = h(b"prototype");
    let info = (*c).info;
    !info.is_null()
        && (*info).code == crate::closure::BOUND_FUNCTION_FUNC_PTR
        && (*c).shape_id != crate::closure::shape::function_dictionary_shape()
        && !matches!(hash, NAME | LENGTH | PROTOTYPE)
}

/// Does a read of `c` for the key hashed `hash`, once its bag proved the key
/// absent, continue at `%Function.prototype%`? True for a plain function body
/// on FunctionDictionary only because its bag holds an accessor
/// (`Array[Symbol.species]`): its own string keys, accessors included, are
/// its bag's, and with no internal state record it has no recorded
/// [[Prototype]] and no deleted marker. The synthesized intrinsic `name`,
/// `length` and `prototype`, and the perf_hooks `supportedEntryTypes`
/// static the complete read synthesizes, decline (#12720).
///
/// # Safety
/// `c` is a live closure cell.
#[inline(never)]
unsafe fn dictionary_function_reads_through(
    c: *const crate::closure::ClosureHeader,
    hash: u64,
) -> bool {
    use crate::object::runtime_layouts::atom_hash as h;
    const NAME: u64 = h(b"name");
    const LENGTH: u64 = h(b"length");
    const PROTOTYPE: u64 = h(b"prototype");
    const SUPPORTED_ENTRY_TYPES: u64 = h(b"supportedEntryTypes");
    let info = (*c).info;
    if info.is_null()
        || (*c).props.is_null()
        || (*c).shape_id != crate::closure::shape::function_dictionary_shape()
        || crate::closure::shape::is_class_info(info)
        || matches!(hash, NAME | LENGTH | PROTOTYPE | SUPPORTED_ENTRY_TYPES)
    {
        return false;
    }
    let code = (*info).code;
    code != crate::closure::BOUND_FUNCTION_FUNC_PTR
        && code != crate::closure::BOUND_METHOD_FUNC_PTR
        && (*info).flags
            & (crate::closure::FN_ASYNC
                | crate::closure::FN_GENERATOR
                | crate::closure::FN_ASYNC_GENERATOR)
            == 0
        && !crate::closure::props::has_state(c as usize)
}

/// [`chain_from`] from an exotic receiver's builtin prototype `next`: there
/// is no ordinary object to resume a complete walk from.
#[inline(never)]
unsafe fn exotic_chain<const ABSENT: bool>(
    dir: *const u8,
    next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    chain_from::<ABSENT>(dir, std::ptr::null(), next, key_bits, hash)
}

/// A plain function's own intrinsic keys, which its layout lists before its
/// bag's (ECMA-262 §10.2.9 SetFunctionLength / §10.2.8 SetFunctionName,
/// §10.2.5 MakeConstructor): `length` and `name`, static data of its body,
/// and a constructor's `prototype`. `None` for any other key (it continues at
/// `%Function.prototype%`); otherwise the own value, or the hole marker when
/// the value is not static data: a name that is not an image string, or a
/// constructor's `prototype`, created on its first read. A bag entry (a
/// redefinition, a materialized `prototype`) was answered before this.
///
/// # Safety
/// `c` is a closure on a described Function shape of a plain body.
#[inline(always)]
unsafe fn function_own_intrinsic(
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    use crate::object::runtime_layouts::atom_hash as h;
    const NAME: u64 = h(b"name");
    const LENGTH: u64 = h(b"length");
    const PROTOTYPE: u64 = h(b"prototype");
    if !matches!(hash, NAME | LENGTH | PROTOTYPE) {
        return None;
    }
    function_own_intrinsic_keyed(c, key_bits, hash)
}

/// The family record of function layout word `word` (the rows below
/// `flat_base`, as `ShapeSlab::receiver_row_record_banded`), or null.
#[inline(always)]
pub(super) unsafe fn family_record_of(dir: *const u8, word: LayoutWord) -> *mut ShapeRecord {
    use crate::codegen_abi::{LAYOUT_BAND_SHIFT, LAYOUT_ROW_MASK, LAYOUT_SHADOW_BIT, LAYOUT_TAG};
    let w = word.0 & !LAYOUT_SHADOW_BIT;
    if w.wrapping_sub(LAYOUT_TAG) >= 3 << LAYOUT_BAND_SHIFT {
        return std::ptr::null_mut();
    }
    let row = (w.wrapping_add(crate::codegen_abi::LAYOUT_ROW_BIAS) & LAYOUT_ROW_MASK) as usize;
    let family = crate::codegen_abi::FUNCTION_FAMILY_ROWS as usize;
    if row >= family {
        return std::ptr::null_mut();
    }
    let base = *(dir.add(FLAT_BASE_OFFSET) as *const *mut ShapeRecord);
    base.wrapping_sub(family).wrapping_add(row)
}

/// The own key `key_bits` (hash `hash`) of bag-less plain function `c`
/// whose family record is `r` (null: not on a family word): the record's
/// INFO entries (`length`, `name`); a keyed record's miss proves the key
/// absent except a constructor's `prototype`.
#[inline(always)]
unsafe fn family_own(
    r: *mut ShapeRecord,
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    use crate::object::runtime_layouts::atom_hash as h;
    const NAME: u64 = h(b"name");
    const LENGTH: u64 = h(b"length");
    const PROTOTYPE: u64 = h(b"prototype");
    // The record's keys are the INFO keys `length` and `name` and, for a
    // constructing family, `prototype`. Any other hash is no own key of a
    // bag-less function: the miss is this one test.
    if !matches!(hash, NAME | LENGTH | PROTOTYPE) {
        return None;
    }
    match family_position(r, key_bits, hash) {
        Ok((pos, load)) => Some(family_hit(c, r, pos, load)),
        // A keyed family record holds every own key of a bag-less
        // function: its miss proves the key is no own key (`prototype` of a
        // non-constructing family; a hash collision with `length`/`name`).
        Err(true) => None,
        Err(false) => function_own_intrinsic(c, key_bits, hash),
    }
}

/// The position of own key `key_bits` (hash `hash`) in family record `r`
/// (null: not on a family word) of a bag-less plain function: `Ok((pos,
/// load))` an own key (its entry is an [`INFO_ENTRY`], [`family_hit`]),
/// `Err(true)` no own key (a keyed family record lists every own key of a
/// bag-less function, in their order), `Err(false)` the record cannot say.
/// The one reader of a family record's keys: reads ([`family_own`],
/// [`closure_bag_own`]) and presence ([`bagless_family_has_own`]).
#[inline(always)]
unsafe fn family_position(
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> Result<(u32, u32), bool> {
    if r.is_null() {
        return Err(false);
    }
    let pw = probe_word(r);
    if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
        return Err(false);
    }
    match list_position(r, pw, key_bits, hash) {
        // An unkeyed record (no key list) proves nothing.
        Err(true) if (*r).logical_key_count == 0 => Err(false),
        found => found,
    }
}

/// Is `key_bits` (hash `hash`) an own key of closure `c`, answered from its
/// family record: `Some(present)` for a bag-less plain function on a keyed
/// family record (`length`, `name` and, for a constructing family,
/// `prototype`, materialized or not), `None` when `c` has a bag or is not on
/// a family record (the caller's general presence check answers).
///
/// # Safety
/// `c` is a live closure cell.
pub(crate) unsafe fn bagless_family_has_own(
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<bool> {
    if crate::closure::props::has_store(c as usize)
        || !crate::closure::shape::closure_on_plain_function_shape(c)
    {
        return None;
    }
    let header = crate::gc::header_from_trusted_user_ptr(c.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return None;
    }
    let r = family_record_of(super::super::ordinary_dir_addr(), (*c).shape_id);
    match family_position(r, key_bits, hash) {
        Ok(_) => Some(true),
        Err(true) => Some(false),
        Err(false) => None,
    }
}

#[inline(never)]
unsafe fn function_own_intrinsic_keyed(
    c: *const crate::closure::ClosureHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    use crate::object::runtime_layouts::atom_hash as h;
    let text: &[u8] = match hash {
        x if x == h(b"name") => b"name",
        x if x == h(b"length") => b"length",
        _ => b"prototype",
    };
    // A hash equal to an intrinsic's is that key only if the texts agree.
    if key_bits & TAG_MASK != STRING_TAG {
        return Some(hole());
    }
    let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
    if (*key).byte_len as usize != text.len()
        || std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            text.len(),
        ) != text
    {
        return None;
    }
    let info = (*c).info;
    Some(match text {
        b"length" => crate::closure::closure_length(c).unwrap_or(0) as f64,
        b"name" => match crate::builtins::function_name_image(info) {
            Some(s) => f64::from_bits(STRING_TAG | s as u64),
            None => hole(),
        },
        _ => {
            // Arrows and non-constructors have no own `prototype`.
            let header = crate::gc::header_from_trusted_user_ptr(c.cast());
            if (*info).flags & (crate::closure::FN_ARROW | crate::closure::FN_NON_CONSTRUCTOR) != 0
                || (*header)._reserved & crate::gc::GC_CLOSURE_NON_CONSTRUCTABLE != 0
            {
                return None;
            }
            hole()
        }
    })
}

/// The builtin prototype object a reserved intrinsic `serial` names, read
/// from its identity word in the agent's word directory (the encoding of
/// runtime prototype families, `layout_desc::PerryProtoFamily`): `None`
/// before the realm named it.
#[inline(always)]
unsafe fn intrinsic_prototype(dir: *const u8, serial: u64) -> Option<*const ObjectHeader> {
    let word_dir = *(dir.add(FLAT_BASE_OFFSET + 24) as *const *const u8);
    if word_dir.is_null() {
        return None;
    }
    let bits = *super::super::shapes_prototype::identity_word_slot_in(word_dir, serial)?;
    if bits & TAG_MASK != POINTER_TAG
        || ((bits & POINTER_MASK) as usize) < perry_abi::RECEIVER_HANDLE_FLOOR
    {
        return None;
    }
    Some((bits & POINTER_MASK) as *const ObjectHeader)
}

/// The record's probe word is not derived yet, or declines.
#[cold]
#[inline(never)]
pub(super) unsafe fn probe_get_underived<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    word: LayoutWord,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let v = match own_position(r, key_bits, hash) {
        Ok(pos) => own_value::<ABSENT>(dir, obj, r, pos, key_bits, hash),
        Err(proven) => {
            let probe = if proven { (*r).probe } else { 0 };
            probe_get_absent::<ABSENT>(dir, obj, word, r, probe, key_bits, hash)
        }
    };
    if ABSENT {
        verify::answer(obj as usize, key_bits, v)
    } else {
        v
    }
}

/// The receiver's layout does not hold the key: its overflow, then its
/// prototype chain. `probe` is the receiver's probe word when its list
/// proved the key absent (`PROBE_ATOMS`), else 0.
#[inline(always)]
unsafe fn probe_get_absent<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    word: LayoutWord,
    r: *mut ShapeRecord,
    probe: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // Every outcome is a tail call or a constant, so the caller's hit path
    // keeps its frameless form.
    if word.0 & crate::object::overflow::SHADOW_BIT != 0 {
        return overflow_then_inherited::<ABSENT>(dir, obj, r, probe, key_bits, hash);
    }
    if probe & PROBE_ATOMS == 0 {
        return hole();
    }
    inherited_get::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// [`probe_get_absent`] for a receiver with an overflow.
#[inline(never)]
pub(super) unsafe fn overflow_then_inherited<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    probe: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    match overflow_get(obj, key_bits, hash) {
        Some(v) => v,
        None if probe & PROBE_ATOMS == 0 => hole(),
        None => inherited_get::<ABSENT>(dir, obj, r, key_bits, hash),
    }
}

/// The overflow entry for `key_bits`: `Some(value)` (for an accessor, its
/// [`pair_answer`]), `None` when the overflow proves the key absent.
/// Inlined at its callers as before the accessor arm existed: that arm is
/// one cold call.
#[inline(always)]
pub(super) unsafe fn overflow_get(
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    // A shadow bit with no overflow (an intrinsic prototype written in
    // place, a relink, a deleted shadow name): the layout's list is every
    // own key.
    if meta.is_null() || (*meta).overflow == 0 {
        return None;
    }
    let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
    let i = crate::object::overflow::find_hashed(t, key_bits, hash)?;
    if *crate::object::overflow::attrs_at(t, i) as u8
        & crate::object::key_attrs::ENTRY_ACCESSOR_MASK
        != 0
    {
        return Some(overflow_accessor_answer(t, obj, i));
    }
    Some(
        crate::object::overflow::get_from_probe(t, obj, i)
            .map(f64::from_bits)
            .unwrap_or_else(hole),
    )
}

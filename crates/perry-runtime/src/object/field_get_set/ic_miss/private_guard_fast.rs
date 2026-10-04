// Allocation-free private-member brand checks (#10501, #11791).
//
// `js_private_guard` runs for every `obj.#x` read and write and every
// `obj.#m()` call. A private field is an `ENTRY_PRIVATE` entry of the
// receiver's key list and a class brand is in its shape's brand list (#11791),
// so both questions the general path asks — does the receiver carry the
// element, in the evaluation the access resolves to — are facts of the
// receiver's ShapeId.
//
// The fast path below answers them for the overwhelmingly common case: an
// INSTANCE member of an ordinary (single-evaluation) class, accessed on an
// ordinary object.
//
//   * Every input the storage namespace depends on is checked to be inert — no
//     fresh ClassDefinitionEvaluation on the lexical brand stack, on the
//     receiver, or on the brand owner. Under exactly those conditions the
//     general path resolves the TEMPLATE namespace (the bare class id). Each of
//     those three inputs can only name an evaluation by reaching a class object
//     whose `class_id` IS the declaring template, so until one has been minted
//     for that template (`note_private_template_evaluated`, from
//     `js_object_mark_class`) none of them needs to be looked at.
//   * The answer is a pure function of `(ShapeId, class, name)`: ids are never
//     reused (see `SHAPE_ID_NEXT`), and a private element is never removed. So
//     a small per-agent cache remembers the proven triples, and a compiled site
//     remembers the last proven ShapeId in its own word.
//
// Whatever the fast path cannot prove it leaves to the general path, which
// remains the single source of every exact verdict and every throw. A fast
// "no" is never an answer, only a fall-through.

/// Distinct private-name spellings seen by the guard, leaked once each.
///
/// Private names are compile-time constants (codegen passes a rodata literal
/// per access site), so this set is bounded by the program text. Interning
/// lets the per-access hint records carry a `&'static str` instead of a fresh
/// `String`, and gives the marker cache a stable identity per spelling.
static PRIVATE_NAME_INTERNER: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> =
    std::sync::Mutex::new(None);

const PRIVATE_NAME_MEMO_SIZE: usize = 64;

crate::perry_thread_local! {
    /// Per-agent memo in front of [`PRIVATE_NAME_INTERNER`], keyed by the
    /// caller's name address. A hit is verified by content, so a recycled
    /// (non-rodata) buffer can only miss, never alias another name.
    static PRIVATE_NAME_MEMO: [std::cell::Cell<(usize, &'static str)>; PRIVATE_NAME_MEMO_SIZE] =
        const { [const { std::cell::Cell::new((0, "")) }; PRIVATE_NAME_MEMO_SIZE] };
}

/// Intern `bytes` as a private-name spelling, or `None` when it is not UTF-8
/// (the general path keeps its historical handling of that case).
pub(crate) fn intern_private_name(bytes: &[u8]) -> Option<&'static str> {
    if bytes.is_empty() {
        return None;
    }
    let address = bytes.as_ptr() as usize;
    let slot = ((address >> 3) ^ bytes.len()) & (PRIVATE_NAME_MEMO_SIZE - 1);
    if let Ok(Some(hit)) = PRIVATE_NAME_MEMO.try_with(|memo| {
        let (cached_address, name) = memo[slot].get();
        (cached_address == address && name.as_bytes() == bytes).then_some(name)
    }) {
        return Some(hit);
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let name = {
        let mut guard = PRIVATE_NAME_INTERNER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let set = guard.get_or_insert_with(std::collections::HashSet::new);
        match set.get(text) {
            Some(&name) => name,
            None => {
                let name: &'static str = Box::leak(text.to_owned().into_boxed_str());
                set.insert(name);
                name
            }
        }
    };
    let _ = PRIVATE_NAME_MEMO.try_with(|memo| memo[slot].set((address, name)));
    Some(name)
}

/// Templates (class ids) of which a class object — a fresh
/// ClassDefinitionEvaluation — has been minted, as a hashed bitset. A set bit
/// only sends that template's accesses to the full inertness checks; a
/// collision costs the same and nothing more. Process-global and monotonic:
/// a deep-copied class object reaching another agent carries a template that
/// was marked before the copy existed.
const PRIVATE_TEMPLATE_BITS: usize = 1 << 16;
///
/// Exported: a compiled private-access site tests its template's bit inline
/// before it trusts its cached ShapeId (codegen `emit_private_site_guard`).
#[no_mangle]
pub static PERRY_PRIVATE_TEMPLATE_EVALUATED: [std::sync::atomic::AtomicU64;
    PRIVATE_TEMPLATE_BITS / 64] =
    [const { std::sync::atomic::AtomicU64::new(0) }; PRIVATE_TEMPLATE_BITS / 64];

#[inline(always)]
fn private_template_bit(class_id: u32) -> (usize, u64) {
    let bit = class_id as usize & (PRIVATE_TEMPLATE_BITS - 1);
    (bit / 64, 1u64 << (bit % 64))
}

/// Record that a class object of template `class_id` now exists. Called
/// wherever an object becomes a class object, before it can be observed.
pub(crate) fn note_private_template_evaluated(class_id: u32) {
    let (word, mask) = private_template_bit(class_id);
    PERRY_PRIVATE_TEMPLATE_EVALUATED[word].fetch_or(mask, std::sync::atomic::Ordering::Relaxed);
}

#[inline(always)]
fn private_template_may_be_evaluated(class_id: u32) -> bool {
    let (word, mask) = private_template_bit(class_id);
    PERRY_PRIVATE_TEMPLATE_EVALUATED[word].load(std::sync::atomic::Ordering::Relaxed) & mask != 0
}

/// One learned `(ShapeId, class, access site name) -> present` fact: the
/// shape lists the private field, or carries the class brand (#11791).
#[derive(Clone, Copy)]
struct PrivateShapeProof {
    shape_id: u32,
    class_id: u32,
    /// The caller's name address. A hit also compares `name`'s bytes, so a
    /// buffer recycled for another spelling can only miss.
    site_name: usize,
    /// The interned spelling, handed back for the hint records.
    name: &'static str,
    /// A field (kind 0) or the class brand (methods and accessors).
    is_field: bool,
}

impl PrivateShapeProof {
    const EMPTY: Self = Self {
        shape_id: 0,
        class_id: 0,
        site_name: 0,
        name: "",
        is_field: false,
    };
}

/// Two-way sets: an entry and the one it displaced, so two hot sites that
/// hash together do not evict each other on every access.
const PRIVATE_PROOF_CACHE_SETS: usize = 128;

crate::perry_thread_local! {
    /// Pointer-free: a miss or an evicted entry only costs one shape lookup.
    /// ShapeId 0 is never a real id, so `EMPTY` cannot match a lookup.
    static PRIVATE_PROOF_CACHE: [[std::cell::Cell<PrivateShapeProof>; 2]; PRIVATE_PROOF_CACHE_SETS] =
        const {
            [const { [const { std::cell::Cell::new(PrivateShapeProof::EMPTY) }; 2] };
                PRIVATE_PROOF_CACHE_SETS]
        };
}

/// Every bit of the site address participates: name literals are byte-aligned
/// and packed, so `"#arr"` and `"#n"` can sit a few bytes apart.
#[inline(always)]
fn private_proof_cache_set(shape_id: u32, class_id: u32, site_name: usize) -> usize {
    let key = ((u64::from(shape_id) << 32) | u64::from(class_id))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (site_name as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    (key >> 57) as usize & (PRIVATE_PROOF_CACHE_SETS - 1)
}

impl PrivateShapeProof {
    #[inline(always)]
    fn matches(&self, shape_id: u32, class_id: u32, name: &[u8], is_field: bool) -> bool {
        self.shape_id == shape_id
            && self.class_id == class_id
            && self.site_name == name.as_ptr() as usize
            && self.is_field == is_field
            && self.name.as_bytes() == name
    }
}

/// The lexical brand stack cannot select a fresh evaluation for `class_id`.
///
/// `current_private_lexical_brand` walks the top entry's pinned heritage only
/// when it is a class object, so a non-pointer top (the `undefined` delimiter
/// every ordinary vtable call pushes) or an empty stack answers `None` without
/// the walk.
#[inline]
fn private_lexical_brand_is_inert(class_id: u32) -> bool {
    let top = PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow().last().copied());
    match top {
        None => true,
        Some(bits) if !JSValue::from_bits(bits).is_pointer() => true,
        Some(_) => current_private_lexical_brand(class_id).is_none(),
    }
}

/// Resolve `obj` to an ordinary shaped heap object, returning it with its
/// ShapeId. `None` means "not proven".
///
/// A plausible heap address is above the handle band, so it is never a Proxy
/// (`proxy::lookup` only decodes ids inside that band). Closures share
/// `GC_TYPE_OBJECT` but not the `ObjectHeader` layout, so they are rejected
/// before the shape word is read.
#[inline]
unsafe fn private_plain_receiver_shape(obj: f64) -> Option<(*const ObjectHeader, u32)> {
    let bits = obj.to_bits();
    if (bits & !crate::value::POINTER_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let object = addr as *const ObjectHeader;
    let shape_id = crate::object::shapes::object_shape_stamp(object);
    (shape_id != 0).then_some((object, shape_id))
}

/// The receiver's own evaluation brand is absent: `private_evaluation_brand`
/// reads this word, and anything that is not a pointer there can never be a
/// class object, so it answers `None`.
#[inline]
unsafe fn private_receiver_has_no_evaluation_brand(object: *const ObjectHeader) -> bool {
    let meta = (*object).meta;
    meta.is_null() || !JSValue::from_bits((*meta).private_evaluation_brand).is_pointer()
}

/// Prove that an INSTANCE private access of `(class_id, name, kind)` on `obj`
/// passes the general path's presence check, from the receiver's shape alone:
/// the shape lists the field's `ENTRY_PRIVATE` entry, or carries the class
/// brand. `None` means "not proven" and must be followed by the general path.
///
/// Success also proves `private_access_owner(brand_owner, class_id)` is
/// `None`, so the caller records hints exactly as the general path would with
/// no owner.
#[inline]
fn private_instance_access_is_proven(
    obj: f64,
    brand_owner: f64,
    class_id: u32,
    name: &[u8],
    kind: u32,
) -> Option<PrivateShapeProof> {
    let (object, shape_id) = unsafe { private_plain_receiver_shape(obj) }?;
    if private_template_may_be_evaluated(class_id) {
        if !unsafe { private_receiver_has_no_evaluation_brand(object) }
            || !private_lexical_brand_is_inert(class_id)
        {
            return None;
        }
        // The brand owner is the receiver itself for every `this.#x`, and then
        // the two conditions above already cover it. A non-pointer owner (a
        // function's `undefined` this, a static ClassRef) carries no
        // evaluation brand. Any other object gets the general path's own two
        // predicates.
        if brand_owner.to_bits() != obj.to_bits()
            && JSValue::from_bits(brand_owner.to_bits()).is_pointer()
            && (private_access_owner(brand_owner, class_id).is_some()
                || private_evaluation_brand_matches(obj, brand_owner, class_id).is_some())
        {
            return None;
        }
    }
    let is_field = kind == 0;
    let set = private_proof_cache_set(shape_id, class_id, name.as_ptr() as usize);
    let cached = PRIVATE_PROOF_CACHE.with(|cache| {
        let [first, second] = &cache[set];
        let first = first.get();
        if first.matches(shape_id, class_id, name, is_field) {
            return Some(first);
        }
        let second = second.get();
        second.matches(shape_id, class_id, name, is_field).then_some(second)
    });
    if cached.is_some() {
        return cached;
    }
    private_shape_proof_learn(object, shape_id, class_id, name, is_field, set)
}

/// Cold half of [`private_instance_access_is_proven`]: read the shape once.
#[cold]
#[inline(never)]
fn private_shape_proof_learn(
    object: *const ObjectHeader,
    shape_id: u32,
    class_id: u32,
    name: &[u8],
    is_field: bool,
    cache_set: usize,
) -> Option<PrivateShapeProof> {
    // Class objects hold static private elements by identity, so only
    // ordinary shapes are proven; the kind is part of the ShapeId, so a cache
    // hit can only be an ordinary object too.
    let interned = intern_private_name(name)?;
    if !crate::object::shapes::shape_object_kind_by_id(shape_id)
        .is_some_and(|kind| kind.is_ordinary_layout())
    {
        return None;
    }
    // The template namespace: the callers established that no fresh
    // evaluation is involved.
    let present = if is_field {
        let storage = private_storage_key_by_id(class_id, 0, interned);
        unsafe { crate::object::key_attrs::object_key_is_private(object, storage.as_bytes()) }
    } else {
        unsafe { crate::object::shapes::object_has_brand(object, private_brand_id(class_id, 0)) }
    };
    if !present {
        return None;
    }
    let learned = PrivateShapeProof {
        shape_id,
        class_id,
        site_name: name.as_ptr() as usize,
        name: interned,
        is_field,
    };
    PRIVATE_PROOF_CACHE.with(|cache| {
        let [first, second] = &cache[cache_set];
        second.set(first.get());
        first.set(learned);
    });
    Some(learned)
}

// ---------------------------------------------------------------------------
// Per-site guard caches.
//
// A compiled access site owns one `i64` (`@perry_private_site_*`, zero
// initialized) holding the last receiver ShapeId it proved (#11791). The
// element's presence is a fact of the shape — a private field is an
// `ENTRY_PRIVATE` entry of its key list, a brand is in its brand list, and
// neither is ever removed — so a hit is a header check and one compare: no
// slot read, no TLS, no hashing, no string.
//
// The template bit is re-checked on every hit, so the first class object of
// that template turns every site of it back to the full checks. The word holds
// no managed address, so the collector never needs to see it, and a racing
// store from another agent can only publish another correct fact — ShapeIds
// are process-global.
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn private_site_hit(obj: f64, class_id: u32, site: *const u64) -> bool {
    if site.is_null() {
        return false;
    }
    let word = (*(site as *const std::sync::atomic::AtomicU64))
        .load(std::sync::atomic::Ordering::Relaxed);
    if word == 0 || private_template_may_be_evaluated(class_id) {
        return false;
    }
    let Some((_, shape_id)) = private_plain_receiver_shape(obj) else {
        return false;
    };
    // A field site's word also carries the slot (see
    // [`private_field_site_word`]); the ShapeId is its low half.
    shape_id == word as u32
}

/// Publish a proven receiver shape to `site`.
#[inline]
unsafe fn private_site_publish(class_id: u32, site: *mut u64, proof: PrivateShapeProof) {
    if site.is_null() || private_template_may_be_evaluated(class_id) {
        return;
    }
    (*(site as *const std::sync::atomic::AtomicU64))
        .store(u64::from(proof.shape_id), std::sync::atomic::Ordering::Relaxed);
}

/// A field site word's lane bit: the field's slot is an `F64` lane of the
/// word's ShapeId, so the compiled store writes a plain finite double there
/// and takes the miss for anything else. Without it the slot is an `Any`
/// lane, stored with the write barrier. Codegen mirrors it
/// (`expr/private_field_site.rs`).
pub(crate) const PRIVATE_FIELD_SITE_F64: u64 = 1 << 48;
/// The slot bits of a field site word: `word >> 32 & PRIVATE_FIELD_SITE_SLOT_MASK`.
pub(crate) const PRIVATE_FIELD_SITE_SLOT_MASK: u64 = 0xFFFF;

/// The word a compiled INSTANCE FIELD site keeps for receivers carrying
/// `shape_id` (#11791): `ShapeId | slot << 32 | lane bit`, or 0 when the
/// field is not a plain inline slot of that shape. Every input is a fact of
/// the ShapeId (the key list with the entry and its position, the live
/// inline bound, the slot's lane), so the word holds for every receiver the
/// inline compare admits. A lane that is neither `Any` nor `F64` (deprecated
/// or ConstFn) publishes nothing; such receivers keep the runtime path.
unsafe fn private_field_site_word(
    object: *const ObjectHeader,
    shape_id: u32,
    class_id: u32,
    name: &'static str,
) -> u64 {
    if (*object).class_id == NATIVE_MODULE_CLASS_ID
        || crate::object::dictionary::is_dictionary(object)
    {
        return 0;
    }
    let Some(d) = crate::object::shapes::shape_descriptor_by_id(shape_id) else {
        return 0;
    };
    if !d.object_kind.is_ordinary_layout() {
        return 0;
    }
    let keys = d.keys_view();
    if keys.is_null() {
        return 0;
    }
    let storage = private_storage_key_by_id(class_id, 0, name);
    let Some(slot) =
        crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), storage.as_bytes())
    else {
        return 0;
    };
    if crate::object::key_attrs::keys_entry(keys.arr(), slot)
        != crate::object::key_attrs::PRIVATE_FIELD_ENTRY
        || slot >= d.live_inline_slot_count
        || u64::from(slot) > PRIVATE_FIELD_SITE_SLOT_MASK
    {
        return 0;
    }
    let lane = if slot < crate::object::field_rep::REP_SLOTS {
        crate::object::field_rep::slot_rep(d.rep, slot)
    } else {
        crate::object::field_rep::REP_ANY
    };
    let lane_bit = match lane {
        crate::object::field_rep::REP_ANY => 0,
        crate::object::field_rep::REP_F64 => PRIVATE_FIELD_SITE_F64,
        _ => return 0,
    };
    u64::from(shape_id) | (u64::from(slot) << 32) | lane_bit
}

/// Publish a proven receiver shape to a FIELD site, with its slot and lane.
#[inline]
unsafe fn private_field_site_publish(
    obj: f64,
    class_id: u32,
    site: *mut u64,
    proof: PrivateShapeProof,
) {
    if site.is_null() || private_template_may_be_evaluated(class_id) {
        return;
    }
    let Some((object, shape_id)) = private_plain_receiver_shape(obj) else {
        return;
    };
    if shape_id != proof.shape_id {
        return;
    }
    let word = private_field_site_word(object, shape_id, class_id, proof.name);
    if word != 0 {
        (*(site as *const std::sync::atomic::AtomicU64))
            .store(word, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The brand check of a compiled INSTANCE FIELD site's miss: prove the
/// receiver and publish the site word, or run the general guard, which
/// throws or records the hint the by-name access after it consumes.
#[inline]
fn private_field_site_check(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    op: u32,
    site: *mut u64,
) {
    if declaring_class_id != 0 && !field_name_ptr.is_null() && field_name_len != 0 {
        let name = unsafe { std::slice::from_raw_parts(field_name_ptr, field_name_len as usize) };
        if let Some(proof) =
            private_instance_access_is_proven(obj, brand_owner, declaring_class_id, name, 0)
        {
            unsafe { private_field_site_publish(obj, declaring_class_id, site, proof) };
            return;
        }
    }
    js_private_guard(
        obj,
        brand_owner,
        declaring_class_id,
        field_name_ptr,
        field_name_len,
        0,
        op,
    );
}

/// Miss of a compiled `recv.#x` read whose hit is the inline slot load
/// (#11791): the brand check, then the by-name read of the field's storage
/// key `key` (a heap string), which also resolves a fresh evaluation's
/// storage.
#[no_mangle]
pub extern "C" fn js_private_field_site_get(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    site: *mut u64,
    key: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    let key = scope.root_nanbox_f64(key);
    private_field_site_check(
        obj.get_nanbox_f64(),
        brand_owner,
        declaring_class_id,
        field_name_ptr,
        field_name_len,
        0,
        site,
    );
    let key_ptr = (key.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK)
        as *const crate::StringHeader;
    js_object_get_field_by_name_boxed(obj.get_nanbox_f64(), key_ptr)
}

/// Miss of a compiled `recv.#x = value` whose hit is the inline slot store
/// (#11791): the brand check, then the PutValue of the storage key `key`,
/// which stores into the field (or into a fresh evaluation's storage).
/// Returns `value`.
#[no_mangle]
pub extern "C" fn js_private_field_site_set(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    site: *mut u64,
    key: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    private_field_site_check(
        obj.get_nanbox_f64(),
        brand_owner,
        declaring_class_id,
        field_name_ptr,
        field_name_len,
        1,
        site,
    );
    crate::proxy::js_put_value_set(
        obj.get_nanbox_f64(),
        key.get_nanbox_f64(),
        value.get_nanbox_f64(),
        obj.get_nanbox_f64(),
        1,
    );
    value.get_nanbox_f64()
}

/// [`js_private_guard`] for a compiled INSTANCE FIELD access (`kind` 0,
/// `op` 0/1) that owns a per-site cache word (#10501). A field access records
/// no hint when no fresh evaluation is involved, so a cache hit returns the
/// receiver with no other effect; everything else is `js_private_guard`.
#[no_mangle]
pub extern "C" fn js_private_guard_site(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    kind: u32,
    op: u32,
    site: *mut u64,
) -> f64 {
    if kind == 0 && op < 2 && declaring_class_id != 0 {
        if unsafe { private_site_hit(obj, declaring_class_id, site) } {
            return obj;
        }
        if !field_name_ptr.is_null() && field_name_len != 0 {
            let name =
                unsafe { std::slice::from_raw_parts(field_name_ptr, field_name_len as usize) };
            if let Some(proof) =
                private_instance_access_is_proven(obj, brand_owner, declaring_class_id, name, 0)
            {
                unsafe { private_field_site_publish(obj, declaring_class_id, site, proof) };
                return obj;
            }
        }
    }
    js_private_guard(
        obj,
        brand_owner,
        declaring_class_id,
        field_name_ptr,
        field_name_len,
        kind,
        op,
    )
}

// ---------------------------------------------------------------------------
// Fused `recv.#m(args)` (#10501).
//
// The generic lowering READ the private method — `js_private_guard` recorded a
// hint, the by-name get consumed it and allocated a bound-method closure — and
// then called that closure: ~12,000 instructions per call. A private method
// cannot be overridden, shadowed or replaced, so the compiled call site does
// the brand check (`js_private_method_guard`, before the arguments, as
// PrivateGet requires) and then calls the declaring class's vtable entry
// (`js_private_method_call`, after them), resolving that entry once per site.
// Neither half touches the hint stack, so they cannot be mis-paired with an
// unrelated pending access of the same name.
// ---------------------------------------------------------------------------

/// Brand + initialized check for the receiver of a compiled instance
/// `recv.#m(args)`; returns the receiver to call on or throws `TypeError`.
#[no_mangle]
pub extern "C" fn js_private_method_guard(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    method_name_ptr: *const u8,
    method_name_len: u32,
    site: *mut u64,
) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    if method_name_ptr.is_null() || method_name_len == 0 {
        throw_private_type_error("Invalid private field name");
    }
    if unsafe { private_site_hit(obj, declaring_class_id, site) } {
        return obj;
    }
    let name = unsafe { std::slice::from_raw_parts(method_name_ptr, method_name_len as usize) };
    if let Some(proof) =
        private_instance_access_is_proven(obj, brand_owner, declaring_class_id, name, 1)
    {
        unsafe { private_site_publish(declaring_class_id, site, proof) };
        return obj;
    }
    private_guard_checked(obj, brand_owner, declaring_class_id, name, 1, 0, false)
}

/// `site` layout for [`js_private_method_call`]: the vtable entry's function
/// pointer (0 = unresolved) and its packed ABI facts. Both are process-global
/// and a pure function of `(class, name)`, so concurrent resolutions store
/// identical words.
const PRIVATE_CALL_SITE_SYNTHETIC_ARGUMENTS: u64 = 1 << 32;
const PRIVATE_CALL_SITE_REST: u64 = 1 << 33;

/// Call the declaring class's private method `name` on an already-guarded
/// receiver, with the lexical private-name environment the by-name path
/// established for it.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_private_method_call(
    receiver: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    method_name_ptr: *const u8,
    method_name_len: u32,
    args_ptr: *const f64,
    args_len: usize,
    site: *mut u64,
) -> f64 {
    let words = site as *const std::sync::atomic::AtomicU64;
    let mut func_ptr = if site.is_null() {
        0
    } else {
        (*words).load(std::sync::atomic::Ordering::Relaxed) as usize
    };
    let mut facts = if func_ptr == 0 {
        0
    } else {
        (*words.add(1)).load(std::sync::atomic::Ordering::Relaxed)
    };
    // The lexical owner the guard's general path would have recorded. It can
    // only exist once a class object of this template does.
    let owner = if private_template_may_be_evaluated(declaring_class_id) {
        private_access_owner(brand_owner, declaring_class_id)
    } else {
        None
    };
    let _owner = PrivateHintBrandScope::new(owner);
    if func_ptr == 0 {
        let name = std::slice::from_raw_parts(method_name_ptr, method_name_len as usize);
        let Some(name) = intern_private_name(name) else {
            throw_private_type_error("Invalid private field name");
        };
        let Some((ptr, param_count, has_synthetic_arguments, has_rest)) =
            super::super::class_registry::lookup_class_method_in_chain(declaring_class_id, name)
        else {
            return private_method_call_by_storage_name(
                receiver,
                declaring_class_id,
                name,
                owner,
                args_ptr,
                args_len,
            );
        };
        func_ptr = ptr;
        facts = u64::from(param_count)
            | if has_synthetic_arguments { PRIVATE_CALL_SITE_SYNTHETIC_ARGUMENTS } else { 0 }
            | if has_rest { PRIVATE_CALL_SITE_REST } else { 0 };
        if !site.is_null() {
            (*words.add(1)).store(facts, std::sync::atomic::Ordering::Relaxed);
            (*words).store(func_ptr as u64, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let private_brand = owner
        .and_then(|_| current_private_lexical_brand(declaring_class_id))
        .map(f64::from_bits)
        .or_else(|| private_receiver_evaluation_brand(receiver))
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    let this_bits = receiver.to_bits();
    let this = if (this_bits >> 48) == 0x7FFD {
        (this_bits & crate::value::POINTER_MASK) as i64
    } else {
        this_bits as i64
    };
    super::super::class_registry::call_vtable_method_with_private_brand(
        func_ptr,
        this,
        args_ptr,
        args_len,
        (facts & 0xFFFF_FFFF) as u32,
        facts & PRIVATE_CALL_SITE_SYNTHETIC_ARGUMENTS != 0,
        facts & PRIVATE_CALL_SITE_REST != 0,
        private_brand,
    )
}

/// [`private_evaluation_brand_value`] without its shape-descriptor lookup for
/// the common receiver: an ordinary object (by the per-agent ShapeId kind
/// cache) is not a class object, so only its metadata brand can answer.
#[inline]
fn private_receiver_evaluation_brand(receiver: f64) -> Option<f64> {
    if let Some((object, shape_id)) = unsafe { private_plain_receiver_shape(receiver) } {
        if crate::object::shapes::shape_object_kind_by_id(shape_id)
            .is_some_and(|kind| kind.is_ordinary_layout())
        {
            let meta = unsafe { (*object).meta };
            if meta.is_null() {
                return None;
            }
            let brand = f64::from_bits(unsafe { (*meta).private_evaluation_brand });
            return super::super::class_registry::is_class_object_value(brand).then_some(brand);
        }
    }
    private_evaluation_brand_value(receiver)
}

/// The pre-#10501 route, for a method the vtable does not resolve: record the
/// hint the generic guard would have, then dispatch by storage name.
#[cold]
unsafe fn private_method_call_by_storage_name(
    receiver: f64,
    declaring_class_id: u32,
    name: &'static str,
    owner: Option<u64>,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    private_guard_record_access(declaring_class_id, name, 1, false, false, owner, true);
    let storage = format!("#<perry:private-member:{declaring_class_id}:{name}>");
    crate::object::js_native_call_method(
        receiver,
        storage.as_ptr() as *const i8,
        storage.len(),
        args_ptr,
        args_len,
    )
}

/// Kind/op legality and the hint records that follow a successful brand check
/// (spec order: the brand first, then the operation against the member kind).
/// Shared by the fast and general paths of `js_private_guard`.
#[inline]
fn private_guard_record_access(
    declaring_class_id: u32,
    name: &'static str,
    kind: u32,
    is_static: bool,
    is_write: bool,
    access_owner: Option<u64>,
    record_hints: bool,
) {
    match (is_write, kind) {
        // [[Get]] of an accessor without a getter.
        (false, 3) => throw_private_type_error(&format!("'{name}' was defined without a getter")),
        // [[Set]] of an accessor without a setter.
        (true, 2) => throw_private_type_error(&format!("'{name}' was defined without a setter")),
        (true, 1) => throw_private_type_error(&format!("Private method '{name}' is not writable")),
        _ => {}
    }
    if !record_hints {
        return;
    }
    if kind != 0 || (!is_static && access_owner.is_some()) {
        PRIVATE_MEMBER_ACCESS_HINTS.with(|hints| {
            hints.borrow_mut().push(PrivateMemberAccessHint {
                class_id: declaring_class_id,
                name,
                kind,
                is_static,
                is_write,
                brand_owner: access_owner,
            });
        });
    }
    if kind == 1 && !is_write {
        PRIVATE_METHOD_OWNER_HINT.with(|hint| {
            *hint.borrow_mut() = Some((declaring_class_id, name));
        });
    }
}

#[cfg(test)]
mod private_guard_fast_tests {
    use super::*;

    fn proven(obj: f64, brand_owner: f64, class_id: u32, name: &str, kind: u32) -> bool {
        private_instance_access_is_proven(obj, brand_owner, class_id, name.as_bytes(), kind)
            .is_some()
    }

    #[test]
    fn interning_is_by_content_not_address() {
        let a = String::from("#interned_a");
        let b = String::from("#interned_a");
        let first = intern_private_name(a.as_bytes()).unwrap();
        let second = intern_private_name(b.as_bytes()).unwrap();
        assert_eq!(first, "#interned_a");
        assert!(std::ptr::eq(first, second));
        // A recycled buffer at the memoized address must not answer the old
        // spelling.
        let mut buf = *b"#interned_x";
        let x = intern_private_name(&buf).unwrap();
        buf[10] = b'y';
        let y = intern_private_name(&buf).unwrap();
        assert_eq!(x, "#interned_x");
        assert_eq!(y, "#interned_y");
        assert_eq!(intern_private_name(&[0xFF, 0xFE]), None);
    }

    /// An instance of `cid` carrying private `fields` (added as the class
    /// would) and, when `brand`, the class brand (#11791).
    unsafe fn instance_with(cid: u32, fields: &[&str], brand: bool) -> f64 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(cid, 0) as i64,
        ));
        if brand {
            js_private_brand_add(obj.get_nanbox_f64(), cid);
        }
        for field in fields {
            let key = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
            js_private_field_add(
                obj.get_nanbox_f64(),
                cid,
                crate::value::js_nanbox_string(key as i64),
                1.0,
            );
        }
        obj.get_nanbox_f64()
    }

    /// The fast path agrees with the general presence check for present and
    /// absent fields and brands, before and after its cache is warm.
    #[test]
    fn fast_path_matches_the_general_presence_check() {
        unsafe {
            const CID: u32 = 62_601;
            const OTHER: u32 = 62_602;
            let name = intern_private_name(b"#fast").unwrap();
            let branded = instance_with(CID, &["#fast"], true);
            let plain = instance_with(CID, &[], false);
            for _ in 0..3 {
                assert!(proven(branded, branded, CID, name, 0));
                assert!(proven(branded, branded, CID, name, 1));
                assert!(!proven(branded, branded, OTHER, name, 0));
                assert!(!proven(branded, branded, OTHER, name, 1));
                assert!(!proven(plain, plain, CID, name, 0));
                assert!(!proven(plain, plain, CID, name, 1));
            }
            for (obj, kind) in [(branded, 0), (branded, 1), (plain, 0), (plain, 1)] {
                let general = private_instance_element_is_present(
                    obj,
                    CID,
                    name.as_ptr(),
                    name.len() as u32,
                    kind,
                );
                let fast = proven(obj, obj, CID, name, kind);
                assert!(!fast || general, "fast path proved an access the general path rejects");
                assert_eq!(fast, general);
            }
        }
    }

    /// The brand is a fact of the shape: two objects with the same keys
    /// differ in ShapeId by their brands, every later transition carries the
    /// brand, and a second install is refused.
    #[test]
    fn brand_is_a_shape_fact_carried_by_transitions() {
        unsafe {
            const CID: u32 = 62_605;
            let scope = crate::gc::RuntimeHandleScope::new();
            let branded = scope.root_nanbox_f64(instance_with(CID, &[], true));
            let plain = scope.root_nanbox_f64(instance_with(CID, &[], false));
            let shape_of = |value: f64| {
                crate::object::shapes::object_shape_stamp(
                    JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>(),
                )
            };
            assert_ne!(shape_of(branded.get_nanbox_f64()), shape_of(plain.get_nanbox_f64()));
            assert_eq!(
                crate::object::shapes::shape_brands_by_id(shape_of(branded.get_nanbox_f64())),
                Some(&[u64::from(CID)][..])
            );
            let key = crate::string::intern_ascii_literal(b"later");
            js_object_set_field_by_name(
                JSValue::from_bits(branded.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>()
                    as *mut ObjectHeader,
                key,
                3.0,
            );
            assert!(
                crate::object::shapes::object_has_brand(
                    JSValue::from_bits(branded.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>(),
                    u64::from(CID)
                ),
                "a key add must carry the brand"
            );
            assert!(!crate::object::shapes::transition_object_shape_add_brand(
                JSValue::from_bits(branded.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>()
                    as *mut ObjectHeader,
                u64::from(CID)
            ));
        }
    }

    /// Non-objects, and a receiver carrying a fresh evaluation brand, are
    /// never proven here — they belong to the general path.
    #[test]
    fn non_objects_and_evaluation_branded_receivers_fall_through() {
        unsafe {
            const CID: u32 = 62_604;
            let name = intern_private_name(b"#eval").unwrap();
            for value in [
                f64::from_bits(crate::value::TAG_UNDEFINED),
                f64::from_bits(crate::value::TAG_NULL),
                1.5,
            ] {
                assert!(!proven(value, value, CID, name, 0));
            }
            let obj = instance_with(CID, &["#eval"], false);
            assert!(proven(obj, obj, CID, name, 0));
            let class = crate::object::js_object_alloc(CID, 0);
            crate::object::class_registry::js_object_mark_class(class as i64);
            let class_value = crate::value::js_nanbox_pointer(class as i64);
            stamp_private_evaluation_brand(
                JSValue::from_bits(obj.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                class_value,
            );
            assert!(!proven(obj, obj, CID, name, 0));
        }
    }

    /// The per-site word learns the receiver's ShapeId for inline and
    /// overflow fields alike, answers on the shape word alone, misses for a
    /// shape without the field, and stops answering the moment the template
    /// gains a class object.
    #[test]
    fn site_word_publishes_hits_and_refuses_stale_facts() {
        unsafe {
            const CID: u32 = 62_611;
            let mut fields = vec!["#in"];
            let fillers: Vec<String> = (0..16).map(|i| format!("#filler{i}")).collect();
            fields.extend(fillers.iter().map(String::as_str));
            fields.push("#out");
            let obj = instance_with(CID, &fields, false);
            let (_, shape_id) = private_plain_receiver_shape(obj).unwrap();
            let live = crate::object::shapes::shape_live_inline_slot_count_by_id(shape_id).unwrap();
            assert!(live < fields.len() as u32, "fixture must spill its last field");
            let other = instance_with(CID, &[], false);
            for (name, inline) in [(&b"#in"[..], true), (&b"#out"[..], false)] {
                let mut site = 0u64;
                assert!(!private_site_hit(obj, CID, &site));
                let returned = js_private_guard_site(
                    obj,
                    obj,
                    CID,
                    name.as_ptr(),
                    name.len() as u32,
                    0,
                    0,
                    &mut site,
                );
                assert_eq!(returned.to_bits(), obj.to_bits());
                if !inline {
                    // A spilled field is no plain inline slot: no word, and
                    // every access keeps the runtime path (#11791).
                    assert_eq!(site, 0, "a spilled field publishes no site word");
                    continue;
                }
                assert_eq!(site as u32, shape_id, "a proven access publishes its shape");
                assert_eq!(
                    (site >> 32) & PRIVATE_FIELD_SITE_SLOT_MASK,
                    0,
                    "and the field's slot"
                );
                assert!(private_site_hit(obj, CID, &site));
                assert!(
                    !private_site_hit(other, CID, &site),
                    "a shape without the field must not be answered from the site"
                );
            }

            let mut site = 0u64;
            let name = b"#in";
            js_private_guard_site(obj, obj, CID, name.as_ptr(), 3, 0, 1, &mut site);
            assert!(private_site_hit(obj, CID, &site));
            note_private_template_evaluated(CID);
            assert!(
                !private_site_hit(obj, CID, &site),
                "a template with a class object must go back to the full checks"
            );
        }
    }
}

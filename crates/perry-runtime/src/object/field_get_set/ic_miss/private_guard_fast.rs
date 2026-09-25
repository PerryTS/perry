// Allocation-free private-member brand checks (#10501).
//
// `js_private_guard` runs for every `obj.#x` read and write and every
// `obj.#m()` call. Its general path spells the brand marker
// (`#<perry:private-field:…>` or `#<perry:private-brand:…>`) with `format!`,
// resolves the storage namespace, and scans the receiver's own keys by name —
// twice, once as the brand fallback and once as the "initialized" check. That
// was ~4,600 instructions per access against ~60 for a public field, and
// `lru-cache` (≈20 private fields and methods touched per get/set) spent about
// half its CPU there.
//
// The fast path below answers the same two questions without building a
// string, for the overwhelmingly common case: an INSTANCE member of an
// ordinary (single-evaluation) class, accessed on an ordinary object.
//
//   * Every input the storage namespace depends on is checked to be inert — no
//     fresh ClassDefinitionEvaluation on the lexical brand stack, on the
//     receiver, or on the brand owner. Under exactly those conditions the
//     general path resolves the TEMPLATE namespace (the bare class id), both of
//     its checks read the same marker, and each reduces to "the receiver owns
//     that marker with a non-undefined value". Each of those three inputs can
//     only name an evaluation by reaching a class object whose `class_id` IS
//     the declaring template, so until one has been minted for that template
//     (`note_private_template_evaluated`, from `js_object_mark_class`) none of
//     them needs to be looked at.
//   * Which own key carries the marker, and whether that key's slot is inline,
//     are pure functions of the receiver's ShapeId: the ordered key list and
//     the live inline-slot bound are part of it, and ids are never reused (see
//     `SHAPE_ID_NEXT`). So a small per-agent cache maps
//     `(ShapeId, class id, marker)` to that slot and a hit reads it directly,
//     with the same undefined-means-absent rule as
//     `js_object_get_own_field_or_undef`.
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
static PRIVATE_TEMPLATE_EVALUATED: [std::sync::atomic::AtomicU64; PRIVATE_TEMPLATE_BITS / 64] =
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
    PRIVATE_TEMPLATE_EVALUATED[word].fetch_or(mask, std::sync::atomic::Ordering::Relaxed);
}

#[inline(always)]
fn private_template_may_be_evaluated(class_id: u32) -> bool {
    let (word, mask) = private_template_bit(class_id);
    PRIVATE_TEMPLATE_EVALUATED[word].load(std::sync::atomic::Ordering::Relaxed) & mask != 0
}

/// One learned `(ShapeId, class, access site name) -> marker slot` fact.
#[derive(Clone, Copy)]
struct PrivateMarkerSlot {
    shape_id: u32,
    class_id: u32,
    /// The caller's name address. A hit also compares `name`'s bytes, so a
    /// buffer recycled for another spelling can only miss.
    site_name: usize,
    /// The interned spelling, handed back for the hint records.
    name: &'static str,
    /// Field marker (kind 0) or the class brand (methods and accessors).
    is_field: bool,
    /// First own-key index spelling the marker, as the general scan finds it.
    index: u32,
    /// `index <` the shape's live inline-slot bound.
    inline: bool,
}

impl PrivateMarkerSlot {
    const EMPTY: Self = Self {
        shape_id: 0,
        class_id: 0,
        site_name: 0,
        name: "",
        is_field: false,
        index: 0,
        inline: false,
    };
}

/// Two-way sets: an entry and the one it displaced, so two hot sites that
/// hash together do not evict each other on every access.
const PRIVATE_MARKER_CACHE_SETS: usize = 128;

crate::perry_thread_local! {
    /// Pointer-free: a miss or an evicted entry only costs one general-path
    /// scan. ShapeId 0 is never a real id, so `EMPTY` cannot match a lookup.
    static PRIVATE_MARKER_CACHE: [[std::cell::Cell<PrivateMarkerSlot>; 2]; PRIVATE_MARKER_CACHE_SETS] =
        const {
            [const { [const { std::cell::Cell::new(PrivateMarkerSlot::EMPTY) }; 2] };
                PRIVATE_MARKER_CACHE_SETS]
        };
}

/// Every bit of the site address participates: name literals are byte-aligned
/// and packed, so `"#arr"` and `"#n"` can sit a few bytes apart.
#[inline(always)]
fn private_marker_cache_set(shape_id: u32, class_id: u32, site_name: usize) -> usize {
    let key = ((u64::from(shape_id) << 32) | u64::from(class_id))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (site_name as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    (key >> 57) as usize & (PRIVATE_MARKER_CACHE_SETS - 1)
}

impl PrivateMarkerSlot {
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
/// (`proxy::lookup` only decodes ids inside that band) and
/// `private_element_receiver(obj)` is `obj` itself. Closures share
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
    if *((addr as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32)
        == crate::closure::CLOSURE_MAGIC
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

/// First own-key index of `obj` whose key spells `marker`, exactly as
/// `js_object_get_own_field_or_undef` scans (same first match, and the same
/// refusal of a keys edge that is not a live dense array).
unsafe fn private_marker_key_index(obj: *const ObjectHeader, marker: &[u8]) -> Option<u32> {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let key_count = keys_view.count() as usize;
    if key_count > (*keys).capacity as usize || key_count > 65536 {
        return None;
    }
    let key_slots =
        crate::array::array_elements_ptr(keys as *const crate::array::ArrayHeader) as *const f64;
    (0..key_count)
        .find(|&i| {
            let key = JSValue::from_bits((*key_slots.add(i)).to_bits());
            crate::string::js_string_key_matches_bytes(key, marker)
        })
        .map(|i| i as u32)
}

/// `js_object_get_own_field_or_undef(obj, marker) != undefined` for a slot
/// already located: inline slots read the field region (the null-pointer
/// word reads as undefined there too), the rest read overflow storage, which
/// filters undefined itself.
#[inline]
unsafe fn private_marker_slot_is_present(obj: *const ObjectHeader, slot: PrivateMarkerSlot) -> bool {
    if slot.inline {
        let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        let bits = *fields.add(slot.index as usize);
        bits != crate::value::TAG_UNDEFINED && bits != crate::value::POINTER_TAG
    } else {
        overflow_get(obj as usize, slot.index as usize).is_some()
    }
}

/// Prove that an INSTANCE private access of `(class_id, name, kind)` on `obj`
/// passes both the brand check and the "initialized" check of the general
/// path, without spelling the marker. Returns the marker slot (carrying the
/// interned name) on success; `None` means "not proven" and must be followed
/// by the general path.
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
) -> Option<PrivateMarkerSlot> {
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
    let set = private_marker_cache_set(shape_id, class_id, name.as_ptr() as usize);
    let cached = PRIVATE_MARKER_CACHE.with(|cache| {
        let [first, second] = &cache[set];
        let first = first.get();
        if first.matches(shape_id, class_id, name, is_field) {
            return Some(first);
        }
        let second = second.get();
        second.matches(shape_id, class_id, name, is_field).then_some(second)
    });
    if let Some(cached) = cached {
        return unsafe { private_marker_slot_is_present(object, cached) }.then_some(cached);
    }
    private_marker_slot_learn(object, shape_id, class_id, name, is_field, set)
}

/// Cold half of [`private_instance_access_is_proven`]: spell the marker once
/// and find its slot by the general scan.
#[cold]
#[inline(never)]
fn private_marker_slot_learn(
    object: *const ObjectHeader,
    shape_id: u32,
    class_id: u32,
    name: &[u8],
    is_field: bool,
    cache_set: usize,
) -> Option<PrivateMarkerSlot> {
    // Class objects resolve their own brand as a static evaluation, so only
    // ordinary shapes are cached; the kind is part of the ShapeId, so a cache
    // hit can only be an ordinary object too.
    let interned = intern_private_name(name)?;
    if crate::object::shapes::shape_object_kind_by_id(shape_id)
        != Some(crate::object::shapes::ShapeObjectKind::Ordinary)
    {
        return None;
    }
    let spelled = if is_field {
        format!("#<perry:private-field:{class_id}:{interned}>")
    } else {
        format!("#<perry:private-brand:{class_id}>")
    };
    let index = unsafe { private_marker_key_index(object, spelled.as_bytes()) }?;
    let live = crate::object::shapes::shape_live_inline_slot_count_by_id(shape_id).unwrap_or(0);
    let learned = PrivateMarkerSlot {
        shape_id,
        class_id,
        site_name: name.as_ptr() as usize,
        name: interned,
        is_field,
        index,
        inline: index < live,
    };
    if !unsafe { private_marker_slot_is_present(object, learned) } {
        return None;
    }
    PRIVATE_MARKER_CACHE.with(|cache| {
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
// initialized) holding `(ShapeId << 32) | overflow bit | (marker slot + 1)` for
// the last receiver shape it proved, published only when the marker slot holds
// `true` — which is what every runtime-written marker holds. A hit is a
// shape-word compare plus one slot read (inline, or the object's overflow
// storage): no TLS, no hashing, no string.
//
// It is exactly as strong as the proof that primed it: the ShapeId fixes the
// key order and the inline bound (so the same slot spells the same marker), a
// `true` value is present by the general path's undefined-means-absent rule,
// and the template bit is re-checked on every hit, so the first class object
// of that template turns every site of it back to the full checks. The word
// holds no managed address, so the collector never needs to see it, and a
// racing store from another agent can only publish another correct fact —
// ShapeIds are process-global.
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
    let Some((object, shape_id)) = private_plain_receiver_shape(obj) else {
        return false;
    };
    if shape_id != (word >> 32) as u32 {
        return false;
    }
    let index = (word & PRIVATE_SITE_INDEX_MASK) as usize - 1;
    if word & PRIVATE_SITE_OVERFLOW != 0 {
        return overflow_get(object as usize, index) == Some(crate::value::TAG_TRUE);
    }
    let fields = (object as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index) == crate::value::TAG_TRUE
}

/// Site-word flag: the marker slot is in overflow storage, not inline.
const PRIVATE_SITE_OVERFLOW: u64 = 1 << 31;
const PRIVATE_SITE_INDEX_MASK: u64 = PRIVATE_SITE_OVERFLOW - 1;

/// Publish a proven marker slot to `site` when a later hit can re-derive the
/// same verdict from the shape word and one inline load.
#[inline]
unsafe fn private_site_publish(obj: f64, class_id: u32, site: *mut u64, slot: PrivateMarkerSlot) {
    if site.is_null() || private_template_may_be_evaluated(class_id) {
        return;
    }
    let object = (obj.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    let marker = if slot.inline {
        let fields = (object as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        Some(*fields.add(slot.index as usize))
    } else {
        overflow_get(object as usize, slot.index as usize)
    };
    // Key indices are bounded by 65536, far below the flag bit.
    if marker != Some(crate::value::TAG_TRUE) || u64::from(slot.index) + 1 > PRIVATE_SITE_INDEX_MASK
    {
        return;
    }
    let word = (u64::from(slot.shape_id) << 32)
        | if slot.inline { 0 } else { PRIVATE_SITE_OVERFLOW }
        | (u64::from(slot.index) + 1);
    (*(site as *const std::sync::atomic::AtomicU64))
        .store(word, std::sync::atomic::Ordering::Relaxed);
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
            if let Some(slot) =
                private_instance_access_is_proven(obj, brand_owner, declaring_class_id, name, 0)
            {
                unsafe { private_site_publish(obj, declaring_class_id, site, slot) };
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
    if let Some(slot) =
        private_instance_access_is_proven(obj, brand_owner, declaring_class_id, name, 1)
    {
        unsafe { private_site_publish(obj, declaring_class_id, site, slot) };
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
            == Some(crate::object::shapes::ShapeObjectKind::Ordinary)
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
    let illegal = matches!(
        (is_write, kind),
        (false, 3) /* read setter-only: [[Get]] of accessor without getter */
            | (true, 2) /* write getter-only: [[Set]] of accessor without setter */
            | (true, 1) /* write private method */
    );
    if illegal {
        throw_private_type_error("Invalid private member operation for its kind");
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

    unsafe fn instance_with_markers(cid: u32, markers: &[&str]) -> f64 {
        let obj = crate::object::js_object_alloc(cid, 0);
        for marker in markers {
            let key = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
            js_object_set_field_by_name(obj, key, f64::from_bits(crate::value::TAG_TRUE));
        }
        crate::value::js_nanbox_pointer(obj as i64)
    }

    /// The fast path agrees with the general marker check for present and
    /// absent fields and brands, before and after its cache is warm.
    #[test]
    fn fast_path_matches_the_general_marker_check() {
        unsafe {
            const CID: u32 = 62_601;
            const OTHER: u32 = 62_602;
            let name = intern_private_name(b"#fast").unwrap();
            let branded = instance_with_markers(
                CID,
                &[
                    "#<perry:private-field:62601:#fast>",
                    "#<perry:private-brand:62601>",
                ],
            );
            let plain = instance_with_markers(CID, &[]);
            for _ in 0..3 {
                assert!(proven(branded, branded, CID, name, 0));
                assert!(proven(branded, branded, CID, name, 1));
                assert!(!proven(branded, branded, OTHER, name, 0));
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

    /// A marker whose value became `undefined` is absent to the general path;
    /// a warm cache entry for that shape must re-read the slot, not trust it.
    #[test]
    fn warm_cache_rereads_the_marker_slot() {
        unsafe {
            const CID: u32 = 62_603;
            let marker = "#<perry:private-field:62603:#slot>";
            let name = intern_private_name(b"#slot").unwrap();
            let obj = instance_with_markers(CID, &[marker]);
            assert!(proven(obj, obj, CID, name, 0));
            let raw = JSValue::from_bits(obj.to_bits()).as_pointer::<ObjectHeader>();
            let key = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
            js_object_set_field_by_name(
                raw as *mut ObjectHeader,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert!(!private_instance_element_is_present(
                obj,
                CID,
                name.as_ptr(),
                name.len() as u32,
                0
            ));
            assert!(!proven(obj, obj, CID, name, 0));
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
            let class = crate::object::js_object_alloc(CID, 0);
            crate::object::class_registry::js_object_mark_class(class as i64);
            let class_value = crate::value::js_nanbox_pointer(class as i64);
            let obj = instance_with_markers(CID, &["#<perry:private-field:62604:#eval>"]);
            assert!(proven(obj, obj, CID, name, 0));
            stamp_private_evaluation_brand(
                JSValue::from_bits(obj.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                class_value,
            );
            assert!(!proven(obj, obj, CID, name, 0));
        }
    }

    /// The per-site word learns inline and overflow marker slots, a warm word
    /// answers on the shape word alone, and it stops answering the moment the
    /// marker stops reading `true` or the template gains a class object.
    #[test]
    fn site_word_publishes_hits_and_refuses_stale_facts() {
        unsafe {
            const CID: u32 = 62_611;
            let inline_marker = "#<perry:private-field:62611:#in>";
            let spilled_marker = "#<perry:private-field:62611:#out>";
            // The second marker sits behind enough filler keys to be past any
            // initial inline capacity, so it lands in overflow storage.
            let mut markers = vec![inline_marker];
            let fillers: Vec<String> = (0..16).map(|i| format!("#<filler:{i}>")).collect();
            markers.extend(fillers.iter().map(String::as_str));
            markers.push(spilled_marker);
            let obj = instance_with_markers(CID, &markers);
            let (_, shape_id) = private_plain_receiver_shape(obj).unwrap();
            let live = crate::object::shapes::shape_live_inline_slot_count_by_id(shape_id).unwrap();
            assert!(live < markers.len() as u32, "fixture must spill its last key");
            for (name, marker, overflow) in [
                (&b"#in"[..], inline_marker, false),
                (&b"#out"[..], spilled_marker, true),
            ] {
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
                assert_ne!(site, 0, "a proven access must publish its site word");
                assert_eq!(site & PRIVATE_SITE_OVERFLOW != 0, overflow);
                assert!(private_site_hit(obj, CID, &site));

                let raw = JSValue::from_bits(obj.to_bits()).as_pointer::<ObjectHeader>();
                let key =
                    crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
                js_object_set_field_by_name(
                    raw as *mut ObjectHeader,
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                assert!(
                    !private_site_hit(obj, CID, &site),
                    "a marker that no longer reads true must not be answered from the site"
                );
                js_object_set_field_by_name(
                    raw as *mut ObjectHeader,
                    key,
                    f64::from_bits(crate::value::TAG_TRUE),
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

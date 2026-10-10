//! Descriptor validation + throw helpers backing `Object.defineProperty` /
//! `Object.create` / `Object.defineProperties` (moved out of `object_ops.rs`).
use super::*;
/// Throw a `TypeError` with the given UTF-8 message bytes. Used by the
/// `Object.defineProperty` / `Object.create` descriptor + invariant validation
/// paths (#2817 / #2843 / #2816).
pub(crate) fn throw_object_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
/// Throw `TypeError: <prefix><suffix>` where `suffix` is a runtime-built
/// string (e.g. the offending descriptor value rendered with the same
/// formatting Node uses in its messages). #2817.
pub(crate) fn throw_object_type_error_with_suffix(prefix: &str, suffix: &str) -> ! {
    let full = format!("{prefix}{suffix}");
    let msg = crate::string::js_string_from_bytes(full.as_ptr(), full.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Render a value the way Node does inside its `Object.defineProperty`
/// descriptor TypeError messages (e.g. `Property description must be an
/// object: 1` / `... : undefined` / `Getter must be a function: 1`).
/// Primitives render via their natural string form; objects render as
/// `[object Object]` etc. — but in practice these error paths only fire on
/// primitives, so a simple coercion suffices.
pub(crate) unsafe fn describe_value_for_type_error(value: f64) -> String {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    let s = crate::value::js_jsvalue_to_string(value);
    if s.is_null() {
        return String::new();
    }
    let len = (*s).byte_len as usize;
    let data = crate::string::string_data(s);
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Is `value` a non-nullish object reference that `Object.defineProperty` /
/// `Object.create` accepts as a descriptor / properties bag? (#2817)
/// Functions/closures count as objects too; a Symbol, or any other cell that
/// holds a primitive, does not.
pub(crate) unsafe fn value_is_object_like(value: f64) -> bool {
    if crate::typedarray_props::typed_array_addr_from_value(value).is_some() {
        return true;
    }
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        // Module-level raw-I64 object pointers (top16 == 0) — accept if it
        // resolves to a real heap object.
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            if is_primitive_cell(bits as usize) {
                return false;
            }
            return is_valid_obj_ptr(bits as *const u8)
                || crate::closure::is_closure_ptr(bits as usize);
        }
        return false;
    }
    let ptr = jv.as_pointer::<u8>() as usize;
    if ptr < 0x10000 {
        return false;
    }
    // A Symbol is pointer-tagged but is a primitive: `Type(sym)` is Symbol,
    // so every caller asking "is this an Object" must hear no. The address
    // window alone admits it, and a define on it then reached the descriptor
    // holder refusal (`HolderEdit::new`) and was dropped instead of throwing.
    if crate::symbol::js_is_symbol(value) != 0 || is_primitive_cell(ptr) {
        return false;
    }
    is_valid_obj_ptr(ptr as *const u8) || crate::closure::is_closure_ptr(ptr)
}

/// Is `addr` a GC cell that holds a primitive (a string, symbol or bigint
/// body)? Such a cell is never a property holder, whatever tag names it.
/// Ownership is proved by allocator metadata before the kind is read.
unsafe fn is_primitive_cell(addr: usize) -> bool {
    crate::value::addr_class::try_read_tracked_gc_header(addr).is_some_and(|header| {
        matches!(
            (*header.as_ptr()).obj_type,
            crate::gc::GC_TYPE_STRING | crate::gc::GC_TYPE_SYMBOL | crate::gc::GC_TYPE_BIGINT
        )
    })
}

/// Is `value` callable (a closure / function) — used to validate `get`/`set`
/// descriptor fields. Per spec, an *omitted* (undefined) accessor is allowed;
/// only a present non-callable value throws. (#2817)
pub(crate) unsafe fn value_is_callable(value: f64) -> bool {
    super::super::value_is_callable(value)
}

pub(crate) unsafe fn registered_buffer_index_own_property_present(
    obj_value: f64,
    key_str: *const crate::StringHeader,
) -> Option<bool> {
    let obj_js = crate::JSValue::from_bits(obj_value.to_bits());
    let raw_buffer_addr = if obj_js.is_pointer() {
        obj_js.as_pointer::<u8>() as usize
    } else {
        let bits = obj_value.to_bits();
        // #10694: a raw word must be allocator-owned before the brand read.
        if bits != 0
            && bits <= 0x0000_FFFF_FFFF_FFFF
            && bits > 0x10000
            && crate::buffer::header_is_owned(bits as usize)
        {
            bits as usize
        } else {
            0
        }
    };
    if raw_buffer_addr == 0 || !crate::buffer::is_registered_buffer(raw_buffer_addr) {
        return None;
    }

    // Only answer for canonical *index* keys here. Non-index keys (e.g.
    // `length` or user-defined expandos on a typed array) are owned by the
    // `typedarray_props` registry — returning `Some(false)` for them would
    // shadow that check (`typed_array_has_own_property`) and wrongly report
    // a defined own property as absent. Fall through with `None` instead.
    let name = super::super::has_own_helpers::str_from_string_header(key_str)?;
    let idx = super::super::canonical_array_index(name)?;
    // #8149: an `ArrayBuffer` / `SharedArrayBuffer` / `DataView` has NO
    // integer-indexed own properties — `Object.prototype.hasOwnProperty
    // .call(dv, "0")` and `getOwnPropertyDescriptor(dv, "0")` are `false` /
    // `undefined` in node. Asked ABOVE the bounds test, which answers `true`
    // for every in-range index. An index STORE does create an ordinary own
    // property, so consult the expando table rather than answering a flat
    // `false`.
    if crate::buffer::is_non_indexed_buffer_view(raw_buffer_addr) {
        return Some(crate::buffer::buffer_has_own_prop(raw_buffer_addr, name));
    }
    let buf = raw_buffer_addr as *const crate::buffer::BufferHeader;
    Some(idx < crate::buffer::store::raw_length(buf as usize))
}

/// `ToPropertyDescriptor` field presence: `HasProperty(descriptor, name)` —
/// own OR inherited. Spec §6.2.6.5 reads each descriptor field with
/// `HasProperty` then `Get`, so an inherited `value`/`get`/... counts as
/// present (e.g. `Object.defineProperty(o, k, child)` where `child`'s prototype
/// carries `value`). `descriptor_value` is the NaN-boxed descriptor object.
// ─── #6748 follow-up: single-pass descriptor decode ──────────────────────────
// `ToPropertyDescriptor` reads up to 6 fields; the per-field helpers below
// each allocate the field-name string and run a full `HasProperty`/`[[Get]]`
// (absent fields walk the prototype chain), so one defineProperty paid ~10
// such probes. For the overwhelmingly-common descriptor — a plain object
// literal with the default prototype and no accessor-backed fields — a single
// walk of its own keys answers everything. `try_decode_descriptor` returns
// `None` whenever any spec-visible subtlety could apply (closure/exotic/class
// receivers, custom [[Prototype]], accessor-backed fields, a polluted
// `Object.prototype`), and callers keep the general per-field path.

pub(crate) const DESC_VALUE: usize = 0;
pub(crate) const DESC_GET: usize = 1;
pub(crate) const DESC_SET: usize = 2;
pub(crate) const DESC_WRITABLE: usize = 3;
pub(crate) const DESC_ENUMERABLE: usize = 4;
pub(crate) const DESC_CONFIGURABLE: usize = 5;

/// A decoded `ToPropertyDescriptor` result whose six field values are GC roots.
///
/// #7963: the view is built ONCE near the top of `js_object_define_property`
/// and then read at a dozen points spread across the rest of that function —
/// past `ensure_key_in_keys_array`, `clone_closure_rebind_this`,
/// `define_property_force_store_value` and the own-key probes, every one of
/// which can allocate and therefore evacuate. Six raw `JSValue`s in a Rust
/// struct are neither shadow slots nor temp roots nor reachable from any
/// registered scanner, so an evacuating minor could neither keep those values
/// alive nor rewrite them — and the stale word was then *stored into* the
/// receiver (`define_property_force_store_value`) or into the accessor side
/// table. Holding each present field as a [`crate::gc::RuntimeHandle`] puts it
/// on the already-registered runtime-handle mutable root scanner, so `read`
/// hands back the post-collection address.
pub(crate) struct DescView<'scope> {
    present: [bool; 6],
    handles: [Option<crate::gc::RuntimeHandle<'scope>>; 6],
}

impl DescView<'_> {
    #[inline]
    pub(crate) fn has(&self, f: usize) -> bool {
        self.present[f]
    }
    /// Field value, **re-read from its root**; `undefined` when absent
    /// (matching the per-field readers).
    #[inline]
    pub(crate) fn read(&self, f: usize) -> crate::value::JSValue {
        match &self.handles[f] {
            Some(h) => crate::value::JSValue::from_bits(h.get_nanbox_u64()),
            None => crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED),
        }
    }
}

impl DescView<'_> {
    pub(crate) fn has_named(&self, name: &[u8]) -> bool {
        desc_field_index(name).is_some_and(|index| self.has(index))
    }
    pub(crate) fn read_named(&self, name: &[u8]) -> crate::JSValue {
        desc_field_index(name)
            .map(|index| self.read(index))
            .unwrap_or_else(|| crate::JSValue::from_bits(crate::value::TAG_UNDEFINED))
    }
    pub(crate) fn flag(&self, name: &[u8]) -> Option<bool> {
        self.has_named(name)
            .then(|| self.read_named(name).bits() == crate::value::TAG_TRUE)
    }
}

/// Public descriptor entry points also admit legacy raw object operands.
/// Normalize those allocation-free BEFORE rooting or calling collecting code.
/// This is an ABI admission step, never a precise-root address decoder. Saved
/// descriptor fields are JSValues at rest and must not use this conversion: a
/// Number whose bits equal a heap address must remain a Number.
pub(crate) unsafe fn normalize_descriptor_operand(value: f64) -> f64 {
    let bits = value.to_bits();
    // Already boxed operands (including class refs/handles) keep their exact
    // producer encoding. No saved field value is sent through this ABI helper.
    if bits >> 48 >= 0x7FF8 {
        return value;
    }
    // This existing receiver decoder explicitly admits allocator-owned typed
    // arrays in raw-bit AND numeric-address form. Use its positive family proof,
    // never the magnitude of the f64 word, before publishing a JSValue root.
    if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(value) {
        return crate::value::js_nanbox_pointer(crate::value::resolve_forwarding(addr) as i64);
    }
    if bits >> 48 != 0
        || crate::value::addr_class::try_read_tracked_gc_header(bits as usize).is_none()
    {
        return value;
    }
    let current = crate::value::resolve_forwarding(bits as usize);
    let candidate = crate::value::js_nanbox_pointer(current as i64);
    if value_is_object_like(candidate) {
        candidate
    } else {
        // Primitive cells must still fail descriptor/receiver object admission.
        value
    }
}

/// The sole observable bag-to-record boundary. Each Has is followed immediately
/// by its Get; accessor validation short-circuits before later fields.
pub(crate) unsafe fn decode_property_descriptor<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    descriptor: &crate::gc::RuntimeHandle<'_>,
) -> DescView<'scope> {
    if !super::definition_target_is_object(descriptor.get_nanbox_f64()) {
        throw_descriptor_value_error(
            "Property description must be an object: ",
            descriptor.get_nanbox_f64(),
        );
    }
    let mut view = if let Some(view) = try_decode_descriptor(scope, descriptor.get_nanbox_f64()) {
        view
    } else {
        let mut view = DescView {
            present: [false; 6],
            handles: [None; 6],
        };
        for name in [
            b"enumerable".as_slice(),
            b"configurable",
            b"value",
            b"writable",
            b"get",
            b"set",
        ] {
            let index = desc_field_index(name).unwrap();
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            ));
            let key_value =
                || key.with_const_ptr(|key| f64::from_bits(crate::JSValue::string_ptr(key).bits()));
            if crate::object::js_object_has_property(descriptor.get_nanbox_f64(), key_value())
                .to_bits()
                != crate::value::TAG_TRUE
            {
                continue;
            }
            let value =
                crate::object::js_object_get_property_key(descriptor.get_nanbox_f64(), key_value());
            view.present[index] = true;
            let value = if matches!(index, DESC_ENUMERABLE | DESC_CONFIGURABLE | DESC_WRITABLE) {
                f64::from_bits(crate::JSValue::bool(crate::value::js_is_truthy(value) != 0).bits())
            } else {
                value
            };
            view.handles[index] = Some(scope.root_nanbox_f64(value));
            validate_accessor_field(&view, index);
        }
        view
    };
    // Fast plain bags have no observable probes, but flags and error precedence
    // must match the generic decoder.
    for index in [DESC_ENUMERABLE, DESC_CONFIGURABLE, DESC_WRITABLE] {
        if view.has(index) {
            let flag = crate::value::js_is_truthy(f64::from_bits(view.read(index).bits())) != 0;
            view.handles[index] = Some(scope.root_nanbox_u64(crate::JSValue::bool(flag).bits()));
        }
    }
    validate_property_descriptor_view(&view);
    view
}

unsafe fn validate_accessor_field(view: &DescView<'_>, index: usize) {
    if !matches!(index, DESC_GET | DESC_SET) || !view.has(index) {
        return;
    }
    let value = view.read(index);
    if !value.is_undefined() && !value_is_callable(f64::from_bits(value.bits())) {
        throw_descriptor_value_error(
            if index == DESC_GET {
                "Getter must be a function: "
            } else {
                "Setter must be a function: "
            },
            f64::from_bits(value.bits()),
        );
    }
}

/// Formatting storage is released before throwing across Rust frames.
unsafe fn throw_descriptor_value_error(prefix: &str, value: f64) -> ! {
    let rendered = describe_value_for_type_error(value);
    let message = format!("{prefix}{rendered}");
    let string = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    drop(message);
    drop(rendered);
    let error = crate::error::js_typeerror_new(string);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64));
}

/// [[GetOwnProperty]] dispatch returns a completed, fresh reflection record.
/// Copy its own fields without consulting Object.prototype: that prototype is
/// observable for user descriptor bags, but cannot alter an internal record.
pub(crate) unsafe fn decode_own_descriptor_result<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    record: &crate::gc::RuntimeHandle<'_>,
) -> DescView<'scope> {
    let mut view = DescView {
        present: [false; 6],
        handles: [None; 6],
    };
    for name in [
        b"enumerable".as_slice(),
        b"configurable",
        b"value",
        b"writable",
        b"get",
        b"set",
    ] {
        let index = desc_field_index(name).unwrap();
        let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ));
        let present = key
            .with_const_ptr(|key| own_key_present(extract_obj_ptr(record.get_nanbox_f64()), key));
        if present {
            let value = key.with_const_ptr(|key| {
                js_object_get_field_by_name(extract_obj_ptr(record.get_nanbox_f64()), key)
            });
            view.present[index] = true;
            view.handles[index] = Some(scope.root_nanbox_u64(
                if matches!(index, DESC_ENUMERABLE | DESC_CONFIGURABLE | DESC_WRITABLE) {
                    crate::JSValue::bool(
                        crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0,
                    )
                    .bits()
                } else {
                    value.bits()
                },
            ));
        }
    }
    view
}

/// Fresh FromPropertyDescriptor bag; callers retain the immutable record.
pub(crate) unsafe fn descriptor_object_from_view(
    scope: &crate::gc::RuntimeHandleScope,
    descriptor: &DescView<'_>,
) -> f64 {
    let object = scope.root_raw_mut_ptr(js_object_alloc(0, 6));
    for name in [
        b"value".as_slice(),
        b"writable",
        b"get",
        b"set",
        b"enumerable",
        b"configurable",
    ] {
        if descriptor.has_named(name) {
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            ));
            // FromPropertyDescriptor uses CreateDataProperty, so inherited
            // setters must not observe or consume the fresh bag fields.
            object.with_mut_ptr(|object| {
                key.with_const_ptr(|key| {
                    define_property_force_store_value(
                        object,
                        key,
                        f64::from_bits(descriptor.read_named(name).bits()),
                    )
                })
            });
        }
    }
    object.with_mut_ptr::<ObjectHeader, _>(|object| crate::value::js_nanbox_pointer(object as i64))
}

#[inline]
fn desc_field_index(b: &[u8]) -> Option<usize> {
    match b {
        b"value" => Some(DESC_VALUE),
        b"get" => Some(DESC_GET),
        b"set" => Some(DESC_SET),
        b"writable" => Some(DESC_WRITABLE),
        b"enumerable" => Some(DESC_ENUMERABLE),
        b"configurable" => Some(DESC_CONFIGURABLE),
        _ => None,
    }
}

/// Does `Object.prototype` carry any of the 6 descriptor field names (own key
/// or any descriptor/accessor installed on it)? Pollution like
/// `Object.prototype.enumerable = true` is spec-visible through
/// `ToPropertyDescriptor`'s inherited-field reads, so a polluted prototype
/// forces the general path.
pub(super) unsafe fn object_prototype_has_desc_field() -> bool {
    // #9103 follow-up: resolve %Object.prototype% through the memoized,
    // root-scanned prototype-addr cache (one slot load + a forwarding heal)
    // instead of the per-call `globalThis.Object` builtin lookup +
    // `closure_get_dynamic_prop(ctor, "prototype")` walk. On the __export
    // install profile that walk was the single largest per-define term
    // (~2.7us of a ~6us install; the keys scan below is ~0.04us). The cache
    // IS the realm intrinsic — the object `ToPropertyDescriptor` reads
    // inherited fields through — so a program that rebinds
    // `globalThis.Object` no longer perturbs the probe (the intrinsic
    // prototype of a descriptor literal never changes), and GC moves are
    // healed by `scan_prototype_addr_cache_roots_mut` exactly as they are for
    // `object_prototype_addr_matches`.
    let addr = crate::array::object_prototype_addr();
    if addr == 0 {
        return false; // intrinsic not materialized yet — nothing own
    }
    let ptr = addr as *mut ObjectHeader;
    // NOTE: builtin init legitimately installs (non-field-named) descriptors
    // on Object.prototype, so the per-object flag is no signal here. Every own
    // install — data write, defineProperty accessor, builtin getter — mirrors
    // its key into keys_array, so scanning it for the 6 names is sufficient.
    let keys_view = crate::object::object_keys(ptr as *const ObjectHeader);
    let keys = keys_view.arr();
    match crate::value::addr_class::try_read_gc_header(keys as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {}
        Some(_) => return true, // unexpected shape — be conservative
        None => return false,   // no keys array — nothing own
    }
    let key_count = keys_view.count() as usize;
    let (slots, slot_len) = keys_view.dense_slots();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..key_count.min(slot_len) {
        let stored = crate::value::JSValue::from_bits((*slots.add(i)).to_bits());
        if let Some(b) = crate::string::js_string_key_bytes(stored, &mut sso) {
            if desc_field_index(b).is_some() {
                return true;
            }
        }
    }
    false
}

/// Single-pass decode of `descriptor_value`'s 6 `ToPropertyDescriptor` fields.
/// `Some(view)` is exactly equivalent to running `desc_has_field` /
/// `desc_read_field` per field; `None` means the caller must use those.
unsafe fn try_decode_descriptor<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    descriptor_value: f64,
) -> Option<DescView<'scope>> {
    let jv = crate::value::JSValue::from_bits(descriptor_value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let addr = jv.as_pointer::<u8>() as usize;
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return None,
    }
    // RegExp cells are OBJECT-typed exotics; class instances can carry
    // prototype getters named like a field; accessor-backed own fields
    // (`get value() {…}` in the literal) fire on [[Get]]; a custom
    // [[Prototype]] contributes inherited fields. All → general path.
    if super::super::exotic_expando::exotic_expando_kind(addr).is_some() {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    // A nonzero class_id is usually just a LITERAL SHAPE id (every object
    // literal gets one) — only a real class with a prototype surface (vtable
    // methods/getters, `C.prototype.x = …` assignments, or a parent chain)
    // could contribute inherited/accessor-backed descriptor fields. Literal
    // shapes have none of those registries populated, so three cheap misses
    // admit them; any registered surface falls back to the general path.
    let class_id = (*obj).class_id;
    if class_id != 0 {
        if super::super::class_registry::get_parent_class_id(class_id).is_some() {
            return None;
        }
        if crate::object::shapes::identity_prototype_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(class_id),
        ) != 0
        {
            return None;
        }
    }
    if crate::object::descriptor_state::object_has_descriptors(addr) {
        return None;
    }
    if super::super::prototype_chain::object_static_prototype(addr).is_some()
        || (class_id != 0
            && (!super::super::class_registry::synthetic_class_prototype_object(class_id)
                .is_null()
                || !super::super::class_registry::class_decl_prototype_object(class_id).is_null()))
    {
        return None;
    }

    let mut view = DescView {
        present: [false; 6],
        handles: [None; 6],
    };
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    if !keys.is_null() {
        match crate::value::addr_class::try_read_gc_header(keys as usize) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {}
            _ => return None, // corrupted keys slot — let the guarded path cope
        }
        let key_count = keys_view.count() as usize;
        let (slots, slot_len) = keys_view.dense_slots();
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..key_count.min(slot_len) {
            let stored = crate::value::JSValue::from_bits((*slots.add(i)).to_bits());
            if let Some(b) = crate::string::js_string_key_bytes(stored, &mut sso) {
                if let Some(fi) = desc_field_index(b) {
                    if !view.present[fi] {
                        view.present[fi] = true;
                        // Root the field value: the caller reads it back long
                        // after several allocating calls (#7963).
                        view.handles[fi] =
                            Some(scope.root_nanbox_u64(js_object_get_field(obj, i as u32).bits()));
                    }
                }
            }
        }
    }
    // Absent fields may still be inherited through the (default) prototype.
    if !view.present.iter().all(|&p| p) && object_prototype_has_desc_field() {
        return None;
    }
    Some(view)
}

/// `validate_property_descriptor`, view form (see the f64 form below).
pub(crate) unsafe fn validate_property_descriptor_view(view: &DescView<'_>) {
    validate_accessor_field(view, DESC_GET);
    validate_accessor_field(view, DESC_SET);
    if (view.has(DESC_GET) || view.has(DESC_SET))
        && (view.has(DESC_VALUE) || view.has(DESC_WRITABLE))
    {
        throw_object_type_error(b"Invalid property descriptor. Cannot both specify accessors and a value or writable attribute, #<Object>");
    }
}

pub(crate) unsafe fn desc_has_field(descriptor_value: f64, name: &[u8]) -> bool {
    // A function object used as a descriptor (`Object.defineProperty(o, k,
    // funObj)`, test262 15.2.3.6-3-139-1 …) is a closure, not an
    // `ObjectHeader`. `js_object_has_property` can't walk a closure's own
    // dynamic props nor its `[[Prototype]]` (`Function.prototype`), so
    // `ToPropertyDescriptor` would miss an inherited `value`/`get`/… field.
    // Route closures through the closure-aware presence check.
    if let Some(ptr) = closure_ptr_from_value(descriptor_value) {
        if let Ok(key_str) = std::str::from_utf8(name) {
            if super::super::has_own_helpers::closure_own_key_present(ptr, key_str) {
                return true;
            }
            // Inherited from `Function.prototype` (and its own chain).
            let fp = crate::object::builtin_prototype_value("Function");
            if value_is_object_like(fp) {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let key_f64 = crate::value::JSValue::string_ptr(key).bits();
                const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
                return crate::object::js_object_has_property(fp, f64::from_bits(key_f64))
                    .to_bits()
                    == TAG_TRUE;
            }
            return false;
        }
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_f64 = crate::value::JSValue::string_ptr(key).bits();
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    crate::object::js_object_has_property(descriptor_value, f64::from_bits(key_f64)).to_bits()
        == TAG_TRUE
}

/// If `value` is a closure (function object), return its heap pointer. Mirrors
/// the closure-pointer recovery used elsewhere in `js_object_define_property`:
/// closures arrive either NaN-boxed with `POINTER_TAG` (function-local) or as a
/// raw in-range I64 (module-level), and `is_closure_ptr` confirms the magic.
pub(crate) unsafe fn closure_ptr_from_value(value: f64) -> Option<usize> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let raw = if jv.is_pointer() {
        jv.as_pointer::<u8>() as usize
    } else {
        let bits = value.to_bits();
        // #10694: a raw word must be allocator-owned before the brand read.
        if bits != 0
            && bits <= 0x0000_FFFF_FFFF_FFFF
            && bits > 0x10000
            && crate::buffer::header_is_owned(bits as usize)
        {
            bits as usize
        } else {
            0
        }
    };
    if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
        Some(raw)
    } else {
        None
    }
}

/// `Get(descriptor, name)` as a value-level read. For an ordinary object the raw
/// `js_object_get_field_by_name` read is sufficient, but a closure descriptor
/// (`Object.defineProperty(o, k, funObj)`) requires reading its own dynamic
/// props and then walking its `[[Prototype]]` (`Function.prototype`) — Perry's
/// `[[Get]]` for the descriptor's `value`/`get`/`set`/attribute fields. Returns
/// `undefined` when the field is absent.
pub(crate) unsafe fn desc_read_field(descriptor_value: f64, name: &[u8]) -> crate::value::JSValue {
    if let Some(ptr) = closure_ptr_from_value(descriptor_value) {
        if let Ok(key_str) = std::str::from_utf8(name) {
            if super::super::has_own_helpers::closure_own_key_present(ptr, key_str) {
                let v = crate::closure::closure_get_dynamic_prop(ptr, key_str);
                return crate::value::JSValue::from_bits(v.to_bits());
            }
            let fp = crate::object::builtin_prototype_value("Function");
            let fp_ptr = extract_obj_ptr(fp);
            if !fp_ptr.is_null() {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                return js_object_get_field_by_name(fp_ptr as *const ObjectHeader, key);
            }
            return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
    // The descriptor may be ANY object — a Date, array, RegExp, boxed
    // primitive, typed array, class instance — not just a plain `ObjectHeader`.
    // A raw `js_object_get_field_by_name(ptr as ObjectHeader)` bit-casts e.g. a
    // Date's cell to an `ObjectHeader` and segfaults (test262
    // Object/create/15.2.3.5-4-* and defineProperties exotic-descriptor cases).
    // Read through the value-level `[[Get]]`, which dispatches on the receiver's
    // real type and — matching `desc_has_field`'s `HasProperty` and the spec
    // `ToPropertyDescriptor` — walks the prototype chain and fires accessors.
    if !value_is_object_like(descriptor_value) {
        return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_f64 = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
    let v = crate::object::js_object_get_property_key(descriptor_value, key_f64);
    crate::value::JSValue::from_bits(v.to_bits())
}

/// #2843: enforce the ordinary `[[DefineOwnProperty]]` invariants
/// (ECMA-262 10.1.6.3 `ValidateAndApplyPropertyDescriptor`) for
/// `Object.defineProperty`. `obj` is the resolved heap object, `key` the
/// coerced key string. Throws the Node `TypeError` when the definition would
/// violate an invariant; returns normally when the definition is permitted.
///
/// Rules (matching Node v25):
///   - Adding a NEW key to a non-extensible object:
///       `Cannot define property <k>, object is not extensible`
///   - Redefining an EXISTING **non-configurable** key in a way the spec
///     forbids (make it configurable, flip enumerable, switch data↔accessor,
///     re-enable writability, or change the value of a non-writable data
///     property to a different value):
///       `Cannot redefine property: <k>`
///
/// A property is non-configurable either object-wide (the object was frozen or
/// sealed — both drop `configurable` on every existing key) OR individually
/// (`Object.defineProperty(obj, k, { configurable: false })`). Both surface
/// through the per-key descriptor side table, so this validation no longer
/// gates on the object-level flags — an individually non-configurable property
/// on an otherwise-extensible object is validated the same way.
pub(crate) unsafe fn enforce_define_property_invariants(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    key_name: &str,
    descriptor: &DescView<'_>,
) {
    if obj.is_null() || (obj as usize) <= 0x10000 {
        return;
    }
    let gc = gc_header_for(obj);
    let no_extend = (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0;

    // #6743: wide objects answer via the O(1) sidecar; the linear scan is the
    // narrow-object fallback (repeated defines were O(N²) through this check).
    let exists = own_key_present_via_index(obj, key).unwrap_or_else(|| own_key_present(obj, key));

    if !exists {
        // Adding a new property to a non-extensible object always throws.
        if no_extend {
            throw_object_type_error_with_suffix(
                "Cannot define property ",
                &format!("{key_name}, object is not extensible"),
            );
        }
        return;
    }

    // Existing own property. Its configurability comes from the per-key
    // descriptor side table: no entry ⇒ the default `{configurable: true}`
    // applies ⇒ any redefinition is permitted. Frozen/sealed objects and
    // explicit `{configurable: false}` defines both populate the table.
    let Some(attrs) = get_property_attrs(obj as usize, key_name) else {
        return;
    };
    if attrs.configurable() {
        return; // still configurable — redefinition allowed
    }

    // --- ValidateAndApplyPropertyDescriptor: current is non-configurable. ---
    let cur_accessor = get_accessor_descriptor(obj as usize, key_name);
    let cur_value = if cur_accessor.is_none() {
        f64::from_bits(js_object_get_field_by_name(obj as *const ObjectHeader, key).bits())
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    validate_nonconfigurable_redefine(key_name, attrs, cur_accessor, cur_value, descriptor);
}

/// The non-configurable branch of `ValidateAndApplyPropertyDescriptor`, factored
/// so the plain-object, function-object (closure), and symbol-keyed define paths
/// share one spec implementation. `cur_attrs` is the existing property's
/// attributes (already known non-configurable). `cur_accessor` is `Some(_)` for
/// an accessor property (carrying its get/set closure bits) or `None` for a data
/// property whose current value is `cur_value`. Throws `TypeError: Cannot
/// redefine property: <k>` when the redefinition violates an invariant.
pub(crate) unsafe fn validate_nonconfigurable_redefine(
    key_name: &str,
    cur_attrs: PropertyAttrs,
    cur_accessor: Option<AccessorDescriptor>,
    cur_value: f64,
    descriptor: &DescView<'_>,
) {
    if !nonconfigurable_redefine_allowed(cur_attrs, cur_accessor, cur_value, descriptor) {
        throw_object_type_error_with_suffix("Cannot redefine property: ", key_name);
    }
}

/// The shared invariant verdict for Object and Reflect definitions.
#[inline(never)]
pub(crate) unsafe fn nonconfigurable_redefine_allowed(
    cur_attrs: PropertyAttrs,
    cur_accessor: Option<AccessorDescriptor>,
    cur_value: f64,
    descriptor: &DescView<'_>,
) -> bool {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    // The requested record is immutable; current values retain their roots.
    let scope = crate::gc::RuntimeHandleScope::new();
    let cur_value_handle = scope.root_nanbox_f64(cur_value);
    let acc_get_handle = scope.root_nanbox_u64(cur_accessor.map(|a| a.get).unwrap_or(0));
    let acc_set_handle = scope.root_nanbox_u64(cur_accessor.map(|a| a.set).unwrap_or(0));

    let has_field = |name: &[u8]| descriptor.has_named(name);
    let read = |name: &[u8]| descriptor.read_named(name);
    let read_bool = |name: &[u8]| descriptor.flag(name);

    let desc_has_get = has_field(b"get");
    let desc_has_set = has_field(b"set");
    let desc_has_value = has_field(b"value");
    let desc_has_writable = has_field(b"writable");
    let desc_is_accessor = desc_has_get || desc_has_set;
    let desc_is_data = desc_has_value || desc_has_writable;

    // Step 4: a non-configurable property cannot be made configurable, and its
    // enumerability cannot change.
    if read_bool(b"configurable") == Some(true) {
        return false;
    }
    if let Some(want_enum) = read_bool(b"enumerable") {
        if want_enum != cur_attrs.enumerable() {
            return false;
        }
    }

    // A generic descriptor (only enumerable/configurable) imposes no further
    // constraints once the two checks above pass.
    if !desc_is_accessor && !desc_is_data {
        return true;
    }

    // Step: a non-configurable property cannot switch between data and accessor.
    let cur_is_accessor = cur_accessor.is_some();
    if desc_is_accessor != cur_is_accessor {
        return false;
    }

    if let Some(acc) = cur_accessor {
        // Both accessor: `get`/`set` may not change. The stored closures are
        // clones rebound to the receiver (`clone_closure_rebind_this`) but keep
        // the original body, so compare by underlying code address.
        let closure_func_ptr = |bits: u64| -> usize {
            let p = (bits & crate::value::POINTER_MASK) as usize;
            if p >= 0x1000 && crate::closure::is_closure_ptr(p) {
                crate::closure::get_valid_func_ptr(p as *const crate::closure::ClosureHeader)
                    as usize
            } else {
                0
            }
        };
        let _ = acc;
        if desc_has_get {
            let want = read(b"get");
            let want_fp = if want.is_undefined() {
                0
            } else {
                closure_func_ptr(want.bits())
            };
            if want_fp != closure_func_ptr(acc_get_handle.get_nanbox_u64()) {
                return false;
            }
        }
        if desc_has_set {
            let want = read(b"set");
            let want_fp = if want.is_undefined() {
                0
            } else {
                closure_func_ptr(want.bits())
            };
            if want_fp != closure_func_ptr(acc_set_handle.get_nanbox_u64()) {
                return false;
            }
        }
        return true;
    }

    // Both data. A non-writable data property cannot be made writable, and its
    // value cannot change to a different value (SameValue). A still-writable
    // data property allows any value/writable change.
    if !cur_attrs.writable() {
        if read_bool(b"writable") == Some(true) {
            return false;
        }
        if desc_has_value {
            let new_value = f64::from_bits(read(b"value").bits());
            if js_object_is(new_value, cur_value_handle.get_nanbox_f64()).to_bits() != TAG_TRUE {
                return false;
            }
        }
    }
    true
}

pub(crate) unsafe fn descriptor_compatible_with_current(
    current: &DescView<'_>,
    descriptor: &DescView<'_>,
) -> bool {
    if current.flag(b"configurable") == Some(true) {
        return true;
    }
    let accessor = (current.has(DESC_GET) || current.has(DESC_SET)).then(|| AccessorDescriptor {
        get: if current.read(DESC_GET).is_undefined() {
            0
        } else {
            current.read(DESC_GET).bits()
        },
        set: if current.read(DESC_SET).is_undefined() {
            0
        } else {
            current.read(DESC_SET).bits()
        },
    });
    nonconfigurable_redefine_allowed(
        PropertyAttrs::new(
            current.flag(b"writable").unwrap_or(false),
            current.flag(b"enumerable").unwrap_or(false),
            false,
        ),
        accessor,
        f64::from_bits(current.read(DESC_VALUE).bits()),
        descriptor,
    )
}

/// Store a data-property value for `Object.defineProperty`, bypassing the
/// ordinary `[[Set]]` writability / frozen / sealed guards. The spec writes the
/// value via `[[DefineOwnProperty]]`, which is NOT subject to the `[[Set]]`
/// writability check — so redefining a configurable-but-non-writable property's
/// value, or performing a (validation-approved) same-value redefine on a frozen
/// object, must store the value rather than throw `Cannot assign to read only`.
///
/// The object's immutability flags are lifted only across the store. `obj` is
/// rooted so a GC evacuation during the store leaves the flag restore landing
/// on the relocated header. Callers must clear any stale per-key `writable`
/// descriptor first (it is re-applied with the final attributes afterward).
pub(crate) unsafe fn define_property_force_store_value(
    obj: *mut ObjectHeader,
    key_str: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key_str);
    let value_handle = scope.root_nanbox_f64(value);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if obj.is_null() || (obj as usize) <= 0x10000 {
        return;
    }
    let immutability =
        crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
    let gc = gc_header_for(obj);
    let saved = (*gc)._reserved;
    (*gc)._reserved &= !immutability;
    let key_str = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    // `js_object_set_field_by_name` implements [[Set]], including inherited
    // setter lookup. DefineProperty must write the receiver's own slot
    // directly. Ensure the shape entry exists, then locate its parallel value
    // slot and store by index (or through object-owned overflow storage).
    // #7341: `ensure_key_in_keys_array` can grow the keys array, so both the
    // receiver and the key must be re-read from their handles AFTER it. Nested
    // `across_*` expresses that ordering without ever binding a pre-call
    // address: the inner call yields the refreshed key, the outer the
    // refreshed receiver.
    let (key_str, obj_reloaded) = obj_handle.across_mut::<ObjectHeader, _>(|| {
        key_handle
            .across_const::<crate::StringHeader, _>(|| {
                ensure_key_in_keys_array_for_value(obj, key_str)
            })
            .1
    });
    obj = obj_reloaded;
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    if !keys.is_null() {
        let count = (keys_view.count() as usize) as usize;
        let (slots, slot_len) = keys_view.dense_slots();
        for i in 0..count.min(slot_len) {
            let stored = JSValue::from_bits((*slots.add(i)).to_bits());
            if crate::string::js_string_key_matches(stored, key_str)
                && !crate::object::key_attrs::entry_is_private(
                    crate::object::key_attrs::keys_entry(keys, i as u32),
                )
            {
                let live_slots = crate::object::object_live_slot_count(obj) as usize;
                let inline_limit = live_slots.max(crate::object::INLINE_SLOT_FLOOR);
                if i < inline_limit {
                    js_object_set_field(
                        obj,
                        i as u32,
                        JSValue::from_bits(value_handle.get_nanbox_f64().to_bits()),
                    );
                } else {
                    crate::object::overflow_set(
                        obj as usize,
                        i,
                        value_handle.get_nanbox_f64().to_bits(),
                    );
                }
                break;
            }
        }
    }
    // Re-fetch after a possible evacuation, then restore the immutability bits.
    obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| {
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved = ((*gc)._reserved & !immutability) | (saved & immutability);
        }
    });
}

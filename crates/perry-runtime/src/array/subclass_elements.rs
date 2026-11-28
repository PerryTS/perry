//! Elements backing store for `class X extends Array` instances.
//!
//! An Array-subclass instance is an ordinary `GC_TYPE_OBJECT` (`super::subclass`);
//! today its indexed elements and `length` are shape-carried properties, so every
//! `push`/`pop`/`obj[i] = v` is a property-shape transition. Under
//! [`array_subclass_elements_enabled`] the instance instead owns a real
//! `GC_TYPE_ARRAY` in `ObjectMeta.elements` (a traced child edge exactly like
//! `spill`) holding its indexed elements and `length`, and the property entry
//! points route canonical array-index keys and `length` to it.
//!
//! This module owns the edge: the gate, the accessor, installation at
//! construction, and the barriered head write-back after a re-allocating append.
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

use super::subclass::{mutation_receiver_allows_plain_tail, ValidatedObjectReceiver};

/// The elements store is the DEFAULT representation for `class X extends
/// Array` instances; `PERRY_ARRAY_SUBCLASS_ELEMENTS=0` restores the
/// shape-carried form (a bisecting kill switch, not a supported mode).
///
/// Flipped on after: the whole `tests/fixtures/programs/` corpus compiled once and run
/// twice under both settings (1285 binaries, 9 output differences, every one
/// of them nondeterministic output — random bytes, timestamps,
/// `console.time`, a PID, a flaky watcher — each reproducible with the switch
/// untouched); the Array-subclass integration suites green with the store
/// enabled; and, on the wolf-ecs twins, −11.4% (add/remove) and −11.9%
/// (entity cycle), 11/11 pairs in both the 2 s and 50 ms windows. Semantics
/// move TOWARD node: `JSON.stringify` produces the array form, `Object.keys`
/// no longer leaks `length`, and the mutator surface
/// (`sort`/`reverse`/`splice`/`shift`/`unshift`, `length` truncation, holes,
/// spread) becomes node-identical.
#[inline]
pub(crate) fn array_subclass_elements_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = FORCED_REPRESENTATION.with(std::cell::Cell::get) {
        return forced;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || {
        !matches!(
            std::env::var("PERRY_ARRAY_SUBCLASS_ELEMENTS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[cfg(test)]
thread_local! {
    static FORCED_REPRESENTATION: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Pins the representation for one test, whatever the process default is.
///
/// The shape-carried form stays reachable through the kill switch, so its
/// tests (`super::subclass_tests`) name it explicitly rather than rely on the
/// default; the elements tests do the same in the other direction.
#[cfg(test)]
pub(crate) struct ArraySubclassRepresentationGuard(Option<bool>);

#[cfg(test)]
impl ArraySubclassRepresentationGuard {
    /// Indexed elements and `length` are shape-carried properties.
    pub(crate) fn shape_carried() -> Self {
        Self::force(false)
    }

    /// Indexed elements and `length` live in `ObjectMeta.elements`.
    pub(crate) fn elements() -> Self {
        Self::force(true)
    }

    fn force(value: bool) -> Self {
        Self(FORCED_REPRESENTATION.with(|cell| cell.replace(Some(value))))
    }
}

#[cfg(test)]
impl Drop for ArraySubclassRepresentationGuard {
    fn drop(&mut self) {
        FORCED_REPRESENTATION.with(|cell| cell.set(self.0));
    }
}

/// The elements store of a live `GC_TYPE_OBJECT`, or null when it has none
/// (no meta record, or not an elements-backed Array subclass instance; a
/// plain object's index store is `super::plain_elements`').
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
#[inline]
pub(crate) unsafe fn elements_of(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // A1_UNPROVEN: record-first: layout word of the Array-subclass instance (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).flags & crate::object::OBJECT_META_FLAG_PLAIN_ELEMENTS != 0 {
        return std::ptr::null_mut();
    }
    (*meta).elements as *mut ArrayHeader
}

/// Store `elements` as the instance's backing store (a barriered meta-record
/// slot store; `elements` may be null to detach, e.g. on deopt).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` with a meta record.
#[inline]
pub(crate) unsafe fn set_elements_head(obj: *mut ObjectHeader, elements: *mut ArrayHeader) {
    // A1_UNPROVEN: record-first: layout word of the Array-subclass instance (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    debug_assert!(!meta.is_null());
    // GC_STORE_AUDIT(BARRIERED): meta-record child edge, stored exactly as
    // `reserve_object_spill` stores `spill`.
    (*meta).elements = elements as u64;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).elements as *const _ as usize,
        elements as u64,
    );
}

/// Install a fresh elements store of `length` holes on `obj` (the `super(n)`
/// shape: `length = n`, every index absent). Idempotent: an instance that
/// already has a store keeps it.
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
pub(crate) unsafe fn install_elements(obj: *mut ObjectHeader, length: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let (_, obj) =
        // A1_UNPROVEN: an `extends Array` instance from a JS value, no type test (record-first: the receiver layout word; kind-A/B question for S0f).
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(crate::meta_obj_runtime_born!(obj), crate::object::MetaReason::ArraySubclassElements));
    if !elements_of(obj).is_null() {
        return;
    }
    let (elements, obj) = obj_handle
        .across_mut::<ObjectHeader, _>(|| crate::array::js_array_alloc_with_length_exact(length));
    if elements_of(obj).is_null() {
        set_elements_head(obj, elements);
    }
}

// ---------------------------------------------------------------------------
// The property funnel: every ordinary-object entry point that can see an
// elements-backed instance's indexed properties or `length` asks here first.
// ---------------------------------------------------------------------------

/// A property key an elements-backed instance answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementsKey {
    Index(u32),
    Length,
}

/// The live `(object, elements)` pair behind a raw or NaN-box-tagged object
/// address, or `None` for anything that is not an elements-backed instance.
///
/// # Safety
/// `addr` must be a raw user pointer or a POINTER-tagged NaN-box; it is
/// classified before any dereference.
#[inline]
pub(crate) unsafe fn backed(addr: usize) -> Option<(*mut ObjectHeader, *mut ArrayHeader)> {
    let bits = addr as u64;
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else {
        addr
    };
    backed_raw(raw)
}

/// [`backed`] of an address that is already a raw user pointer (never a
/// NaN-box): the same classification without the tag strip, for callers
/// that hold an object pointer.
///
/// # Safety
/// `raw` is a raw user pointer; it is classified before any dereference.
#[inline]
pub(crate) unsafe fn backed_raw(raw: usize) -> Option<(*mut ObjectHeader, *mut ArrayHeader)> {
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = raw as *mut ObjectHeader;
    let elements = elements_of(obj);
    (!elements.is_null()).then_some((obj, elements))
}

/// One object's own elements: `count` words from `ptr`, a hole is
/// `TAG_HOLE`. An Array-subclass instance answers from its backing store
/// (`store`, whose `length` is the exotic `length`); an arguments object from
/// the words after its named record [length, callee, @@iterator] (`store`
/// null, count from the GcHeader size word, `length` an ordinary named key).
/// In an arguments object a word that points at a `GC_TYPE_BOX` is a mapped
/// parameter cell: every element hand-out goes through [`view_get`].
#[derive(Clone, Copy)]
pub(crate) struct ElementsView {
    pub owner: *mut ObjectHeader,
    pub store: *mut ArrayHeader,
    pub ptr: *mut u64,
    pub count: u32,
    /// A plain object's index store (`super::plain_elements`): no `length`,
    /// and its owner's layout names no index key.
    pub plain: bool,
}

impl ElementsView {
    /// Does this view answer `key` itself? `length` only for a store.
    #[inline]
    pub(crate) fn admits(&self, key: ElementsKey) -> bool {
        !matches!(key, ElementsKey::Length) || self.exotic_length()
    }

    /// Does this view own an exotic `length` (an Array subclass's store)?
    #[inline]
    pub(crate) fn exotic_length(&self) -> bool {
        !self.store.is_null() && !self.plain
    }

    /// An inline (arguments) index the elements do not hold (its word is the
    /// hole, or it is past the inline count): its property, if any, is own
    /// key `index`, and the key path answers its descriptor and its delete.
    pub(crate) unsafe fn keyed(&self, key: ElementsKey) -> bool {
        matches!(key, ElementsKey::Index(index)
            if self.store.is_null()
                && (index >= self.count || *self.ptr.add(index as usize) == crate::value::TAG_HOLE))
    }

    #[inline]
    pub(crate) fn store_backed(&self) -> bool {
        !self.store.is_null()
    }

    /// The view of a validated store-backed instance.
    ///
    /// # Safety
    /// `elements` is `owner`'s live store.
    #[inline]
    pub(crate) unsafe fn of_store(owner: *mut ObjectHeader, elements: *mut ArrayHeader) -> Self {
        Self {
            owner,
            store: elements,
            ptr: crate::array::array_elements_ptr(elements) as *mut u64,
            count: (*elements).length,
            plain: false,
        }
    }

    /// The view of a plain object's live index store.
    ///
    /// # Safety
    /// `store` is `owner`'s live plain elements store.
    #[inline]
    pub(crate) unsafe fn of_plain(owner: *mut ObjectHeader, store: *mut ArrayHeader) -> Self {
        Self {
            plain: true,
            ..Self::of_store(owner, store)
        }
    }
}

/// Byte offset of an arguments object's first element from its user pointer:
/// object header (16) plus the three named slots (length, callee, @@iterator).
pub(crate) const ARGUMENTS_ELEMENTS_OFFSET: usize = 16 + 3 * 8;

/// Is `obj` (an object whose header carries `OBJ_FLAG_ARGUMENTS`) laid out
/// with inline elements, i.e. on one of the two arguments kinds?
///
/// # Safety
/// `obj` is a live, unforwarded object.
#[inline]
pub(crate) unsafe fn arguments_inline_layout(obj: *const ObjectHeader) -> bool {
    crate::object::shapes::shape_record_for_word(crate::codegen_abi::LayoutWord(
        (*obj).shape_word(),
    ))
    .is_some_and(|record| {
        record.kind_is(crate::object::shapes::ShapeObjectKind::Arguments)
            || record.kind_is(crate::object::shapes::ShapeObjectKind::ArgumentsStrict)
    })
}

/// The elements view of `addr` (see [`ElementsView`]), or `None`.
///
/// # Safety
/// As [`backed`].
#[inline]
pub(crate) unsafe fn own_elements(addr: usize) -> Option<ElementsView> {
    let bits = addr as u64;
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else {
        addr
    };
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = raw as *mut ObjectHeader;
    if header._reserved & crate::gc::OBJ_FLAG_ARGUMENTS != 0 {
        if let Some(view) = arguments_view(obj, header.size) {
            return Some(view);
        }
    }
    let plain = super::plain_elements::store_of(obj);
    if !plain.is_null() {
        return Some(ElementsView::of_plain(obj, plain));
    }
    let elements = elements_of(obj);
    (!elements.is_null()).then(|| ElementsView::of_store(obj, elements))
}

/// The inline-elements view of a flagged object on an arguments kind, `None`
/// otherwise. Out of line: only arguments objects reach it.
///
/// # Safety
/// `obj` is a live, unforwarded object of GcHeader size `size`.
#[cold]
#[inline(never)]
unsafe fn arguments_view(obj: *mut ObjectHeader, size: u32) -> Option<ElementsView> {
    if !arguments_inline_layout(obj) {
        return None;
    }
    let payload = size as usize - crate::gc::GC_HEADER_SIZE;
    Some(ElementsView {
        owner: obj,
        store: std::ptr::null_mut(),
        ptr: (obj as usize + ARGUMENTS_ELEMENTS_OFFSET) as *mut u64,
        count: (payload.saturating_sub(ARGUMENTS_ELEMENTS_OFFSET) / 8) as u32,
        plain: false,
    })
}

/// [`own_elements`] for a NaN-boxed value.
#[inline]
pub(crate) fn own_elements_value(value: f64) -> Option<ElementsView> {
    if !crate::JSValue::from_bits(value.to_bits()).is_pointer() {
        return None;
    }
    unsafe { own_elements(value.to_bits() as usize) }
}

/// The value word of element `index`, `None` for a hole or out of range. The
/// ONE element reader: a mapped arguments slot (a pointer to a
/// `GC_TYPE_BOX`, never a JS value) reads through its parameter cell.
///
/// # Safety
/// `view` is current (no allocation since [`own_elements`]).
#[inline]
pub(crate) unsafe fn view_get(view: ElementsView, index: u32) -> Option<u64> {
    if index >= view.count {
        return None;
    }
    let word = *view.ptr.add(index as usize);
    if word == crate::value::TAG_HOLE {
        return None;
    }
    if view.store.is_null() && (word >> 48) == 0x7FFD {
        let cell = (word & crate::value::POINTER_MASK) as usize;
        if crate::value::addr_class::try_read_gc_header(cell)
            .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_BOX)
        {
            return Some(crate::r#box::js_box_get(cell as *mut crate::r#box::Box).to_bits());
        }
    }
    Some(word)
}

/// [`backed`] for a NaN-boxed value.
pub(crate) fn backed_value(value: f64) -> Option<(*mut ObjectHeader, *mut ArrayHeader)> {
    let js = crate::JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    unsafe { backed(value.to_bits() as usize) }
}

pub(crate) fn key_of_str(name: &str) -> Option<ElementsKey> {
    if name == "length" {
        return Some(ElementsKey::Length);
    }
    crate::object::canonical_array_index(name).map(ElementsKey::Index)
}

/// # Safety
/// `key` must be null or a live string header.
pub(crate) unsafe fn key_of_header(key: *const crate::StringHeader) -> Option<ElementsKey> {
    if key.is_null() {
        return None;
    }
    // Cheap pre-filter: only a leading ASCII digit (an index) or `l`
    // (`length`) can be an elements key, and no canonical index is longer
    // than ten digits. Every ordinary named property — the common case on
    // these receivers — is rejected on one byte, before the UTF-8 decode and
    // the canonical-index parse below.
    let byte_len = (*key).byte_len as usize;
    if byte_len == 0 || byte_len > 10 {
        return None;
    }
    let first = *crate::object::string_header_payload(key);
    if !first.is_ascii_digit() && first != b'l' {
        return None;
    }
    crate::object::has_own_helpers::str_from_string_header(key).and_then(key_of_str)
}

/// A NaN-boxed property key: a canonical-index number, or a string naming an
/// index or `length`. Symbols and everything else are never elements keys.
pub(crate) fn key_of_value(key: f64) -> Option<ElementsKey> {
    let js = crate::JSValue::from_bits(key.to_bits());
    if js.is_number() {
        let n = js.as_number();
        if n.is_finite() && n >= 0.0 && n < 4_294_967_295.0 && n.fract() == 0.0 {
            return Some(ElementsKey::Index(n as u32));
        }
        return None;
    }
    if js.is_any_string() {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        return unsafe { crate::string::js_string_key_bytes(js, &mut sso) }
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(key_of_str);
    }
    None
}

#[inline]
unsafe fn slot_bits(elements: *const ArrayHeader, index: u32) -> u64 {
    *crate::array::array_elements_ptr(elements as *const ArrayHeader)
        .cast::<u64>()
        .add(index as usize)
}

/// Own value for `key`: `length`, or an in-bounds non-hole element. `None`
/// means "not an own property" — the caller continues with its ordinary
/// lookup (the shape carries no such key, so that reaches the prototype
/// chain exactly as a hole on a plain Array does).
///
/// # Safety
/// `elements` must be the live store of a validated instance.
pub(crate) unsafe fn get_by_key(view: ElementsView, key: ElementsKey) -> Option<f64> {
    match key {
        ElementsKey::Length => view
            .exotic_length()
            .then(|| f64::from((*view.store).length)),
        ElementsKey::Index(index) => view_get(view, index).map(f64::from_bits),
    }
}

/// `[[Set]]` of an elements key: an in-bounds store, an append, a hole-creating
/// extension, or a `length` write (Array semantics: truncate or extend with
/// holes; a non-index `length` is a RangeError). The owner is rooted across
/// the re-allocating cases and the new head is written back.
///
/// # Safety
/// `obj` must be a validated elements-backed instance and `elements` its store.
pub(crate) unsafe fn set_by_key(
    obj: *mut ObjectHeader,
    elements: *mut ArrayHeader,
    key: ElementsKey,
    value: f64,
) {
    match key {
        ElementsKey::Length => set_length(obj, elements, value),
        ElementsKey::Index(index) => {
            let length = (*elements).length;
            if index < length {
                crate::array::js_array_set_f64(elements, index, value);
                return;
            }
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let value_root = scope.root_nanbox_f64(value);
            let (head, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
                if index == length {
                    crate::array::js_array_push_f64(elements, value_root.get_nanbox_f64())
                } else {
                    crate::array::js_array_set_f64_extend(
                        elements,
                        index,
                        value_root.get_nanbox_f64(),
                    )
                }
            });
            if !head.is_null() && head != elements_of(obj) {
                set_elements_head(obj, head);
            }
        }
    }
}

/// `length = n` on an elements-backed instance.
///
/// # Safety
/// As [`set_by_key`].
pub(crate) unsafe fn set_length(
    obj: *mut ObjectHeader,
    elements: *mut ArrayHeader,
    new_length: f64,
) {
    if !new_length.is_finite()
        || new_length < 0.0
        || new_length.fract() != 0.0
        || new_length >= 4_294_967_296.0
    {
        crate::array::array_length_range_error();
    }
    let target = new_length as u32;
    let current = (*elements).length;
    if target <= current {
        // Truncation never allocates.
        crate::array::js_array_set_length_strict(elements, new_length);
        return;
    }
    // Extension: grow through the index store (which returns the head), then
    // punch the written slot back out into a hole.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let (head, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
        crate::array::js_array_set_f64_extend(
            elements,
            target - 1,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        )
    });
    let head = if head.is_null() { elements } else { head };
    if head != elements_of(obj) {
        set_elements_head(obj, head);
    }
    crate::array::js_array_delete(head, target - 1);
}

/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn has_own_key(view: ElementsView, key: ElementsKey) -> bool {
    match key {
        ElementsKey::Length => view.exotic_length(),
        ElementsKey::Index(index) => {
            index < view.count && *view.ptr.add(index as usize) != crate::value::TAG_HOLE
        }
    }
}

/// `delete obj[key]`: an index becomes a hole (1); `length` is
/// non-configurable (0).
///
/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn delete_key(view: ElementsView, key: ElementsKey) -> i32 {
    match key {
        ElementsKey::Length => 0,
        ElementsKey::Index(index) if index >= view.count => 1,
        ElementsKey::Index(index) if !view.store.is_null() => {
            crate::array::js_array_delete(view.store, index)
        }
        ElementsKey::Index(index) => {
            let header = crate::gc::header_from_trusted_user_ptr(view.owner.cast());
            let fixed = crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED;
            if *view.ptr.add(index as usize) != crate::value::TAG_HOLE
                && (*header)._reserved & fixed != 0
            {
                return 0;
            }
            // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is a non-pointer sentinel; a
            // mapped box word is dropped with it (delete unmaps).
            *view.ptr.add(index as usize) = crate::value::TAG_HOLE;
            1
        }
    }
}

/// The present (non-hole) indices, ascending.
///
/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn own_index_keys(view: ElementsView) -> Vec<u32> {
    let mut out = Vec::new();
    for index in 0..view.count {
        if *view.ptr.add(index as usize) != crate::value::TAG_HOLE {
            out.push(index);
        }
    }
    out
}

/// A fresh keys array: the present indices as strings (ascending), then
/// `"length"` when `with_length` (getOwnPropertyNames), then every key of
/// `shape_keys` in order. `shape_keys` and the result are rooted across the
/// string allocations.
///
/// # Safety
/// `elements` must be a live store; `shape_keys` a live array (or null).
pub(crate) unsafe fn prepend_index_keys(
    view: ElementsView,
    shape_keys: *mut ArrayHeader,
    with_length: bool,
) -> *mut ArrayHeader {
    let indices = own_index_keys(view);
    let with_length = with_length && view.exotic_length();
    let scope = crate::gc::RuntimeHandleScope::new();
    let shape_h = scope.root_raw_mut_ptr(shape_keys);
    let out_h = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let push_key = |bytes: &[u8]| {
        let (s, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
        });
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, crate::value::js_nanbox_string(s as i64))
        });
        out_h.set_raw_mut_ptr(grown);
    };
    let shape_len = shape_h.with_mut_ptr(|p: *mut ArrayHeader| {
        if p.is_null() {
            0
        } else {
            crate::array::js_array_length(p)
        }
    });
    // OrdinaryOwnPropertyKeys: every index ascending (the elements' and the
    // shape list's leading index keys merged), then `length`, then the rest.
    let mut pending = indices.into_iter().peekable();
    let mut indices_done = false;
    for i in 0..shape_len {
        if !indices_done {
            let key = shape_h.with_mut_ptr(|p: *mut ArrayHeader| crate::array::js_array_get(p, i));
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let key_index = crate::string::js_string_key_bytes(key, &mut sso)
                .and_then(crate::object::canonical_array_index_bytes);
            match key_index {
                Some(n) => {
                    while let Some(index) = pending.next_if(|&e| e < n) {
                        push_key(index.to_string().as_bytes());
                    }
                }
                None => {
                    indices_done = true;
                    for index in pending.by_ref() {
                        push_key(index.to_string().as_bytes());
                    }
                    if with_length {
                        push_key(b"length");
                    }
                }
            }
        }
        let key = shape_h.with_mut_ptr(|p: *mut ArrayHeader| crate::array::js_array_get(p, i));
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, f64::from_bits(key.bits()))
        });
        out_h.set_raw_mut_ptr(grown);
    }
    if !indices_done {
        for index in pending {
            push_key(index.to_string().as_bytes());
        }
        if with_length {
            push_key(b"length");
        }
    }
    out_h.with_mut_ptr(|p| p)
}

/// The own property descriptor of an elements key, or `None` when the key is
/// not an own property (a hole, an index past `length`).
///
/// # Safety
/// `obj` must be a validated instance and `elements` its live store.
pub(crate) unsafe fn own_property_descriptor(view: ElementsView, key: ElementsKey) -> Option<f64> {
    let value = get_by_key(view, key)?;
    let flags = crate::value::addr_class::try_read_gc_header(view.owner as usize)
        .map_or(0, |h| h._reserved);
    let frozen = flags & crate::gc::OBJ_FLAG_FROZEN != 0;
    // An inline (arguments) element is sealed by the header bit as well.
    let fixed = frozen || (view.store.is_null() && flags & crate::gc::OBJ_FLAG_SEALED != 0);
    Some(match key {
        ElementsKey::Index(_) => {
            crate::object::descriptors::build_data_descriptor(value, !frozen, true, !fixed)
        }
        ElementsKey::Length => {
            crate::object::descriptors::build_data_descriptor(value, !frozen, false, false)
        }
    })
}

/// Leave the elements representation for good: every present element becomes
/// a shape-carried index property (ascending), then `length`, and the store is
/// detached. Everything after this runs on the shape-carried machinery
/// (`super::subclass`, `object/array_tail_transition.rs`) exactly as before.
/// Called by the exotic operations that machinery already models —
/// `defineProperty` on an index or `length`, accessor installs, freeze/seal/
/// preventExtensions, `setPrototypeOf` — before they do their work.
///
/// # Safety
/// `obj` must be a validated elements-backed instance.
pub(crate) unsafe fn deopt_to_shape(obj: *mut ObjectHeader) {
    let elements = elements_of(obj);
    if elements.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let elements_h = scope.root_raw_mut_ptr(elements);
    // Detach first: the index stores below must not be routed back here.
    set_elements_head(obj, std::ptr::null_mut());
    let length = (*elements).length;
    for index in own_index_keys(ElementsView::of_store(obj, elements)) {
        let value =
            elements_h.with_mut_ptr(|e: *mut ArrayHeader| f64::from_bits(slot_bits(e, index)));
        let name = index.to_string();
        let (key, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
        });
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
    let (key, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
        crate::string::js_string_from_bytes(b"length".as_ptr(), 6)
    });
    crate::object::set_field_by_name_object_tail(obj, key, f64::from(length));
}

/// [`deopt_to_shape`] for a NaN-boxed receiver that may not be elements-backed.
pub(crate) fn deopt_value(value: f64) {
    if let Some((obj, _)) = backed_value(value) {
        unsafe { deopt_to_shape(obj) };
        return;
    }
    super::plain_elements::leave_value(value);
}

// ---------------------------------------------------------------------------
// Hot-entry helpers used by `super::subclass` (moved here to keep that file
// under the size gate).
// ---------------------------------------------------------------------------

/// The elements store of an elements-backed instance (`super::subclass_elements`),
/// or `None` for the shape-carried representation. Non-null only when the
/// store was installed at construction, so no gate check is needed here.
#[inline]
pub(super) fn elements_for_validated(
    receiver: &ValidatedObjectReceiver,
) -> Option<*mut ArrayHeader> {
    let elements = unsafe { elements_of(receiver.object) };
    (!elements.is_null()).then_some(elements)
}

/// In-bounds, non-hole element of the inner array; `None` sends the caller
/// to the same prototype-chain fallback a hole in the shape-carried form does.
#[inline]
pub(super) fn elements_index_get(elements: *const ArrayHeader, index: u32) -> Option<f64> {
    unsafe {
        if index >= (*elements).length {
            return None;
        }
        let slot = crate::array::array_elements_ptr(elements as *const ArrayHeader)
            .cast::<u64>()
            .add(index as usize);
        let bits = *slot;
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }
}

/// Elements-backed `receiver[index] = value` for an in-bounds index or the
/// appending index (`== length`); anything else (holes past the end, a
/// frozen/sealed/non-extensible receiver) declines to the generic path.
pub(super) fn elements_index_set(
    receiver: &ValidatedObjectReceiver,
    index: u32,
    value: f64,
) -> Option<bool> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return Some(false);
    }
    let length = unsafe { (*elements).length };
    if index < length {
        crate::array::js_array_set_f64(elements, index, value);
        return Some(true);
    }
    if index == length {
        return elements_push(receiver, value).map(|_| true);
    }
    Some(false)
}

/// Elements-backed append: the owner is rooted across the (possibly
/// re-allocating) push and the new head is written back through the
/// barriered meta slot. Returns the new length.
pub(super) fn elements_push(receiver: &ValidatedObjectReceiver, value: f64) -> Option<f64> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return None;
    }
    // In-capacity append: no allocation, therefore no rooting, no head
    // write-back, and none of the receiver re-classification
    // `js_array_push_f64` owes an arbitrary caller-supplied pointer (proxy
    // probe, forwarding-stub cleaning, flag resolution) — this store is ours,
    // reached through the meta slot, and its header is one read away. Only the
    // element bookkeeping (`store_array_slot_resolved`) is kept: it maintains
    // the numeric/element-shape proofs the read tiers and the loop guard
    // consume.
    if let Some(length) = unsafe { elements_push_in_capacity(elements, value) } {
        return Some(length);
    }
    let obj = receiver.object as *mut ObjectHeader;
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let value_root = scope.root_nanbox_f64(value);
        let (grown, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
            crate::array::js_array_push_f64(elements, value_root.get_nanbox_f64())
        });
        let current = elements_of(obj);
        if grown != current {
            set_elements_head(obj, grown);
        }
        Some(f64::from((*grown).length))
    }
}

/// Elements-backed `pop`: the inner array's own pop (no allocation, so no
/// rooting), declining like `elements_push` on a non-plain receiver.
pub(super) fn elements_pop(receiver: &ValidatedObjectReceiver) -> Option<f64> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return None;
    }
    // The mirror of `elements_push_in_capacity`: removing the tail stores
    // nothing, so a non-hole element is a length decrement and a load. An
    // empty store, a hole (which reads through the prototype chain) and every
    // exotic flag keep the complete runtime entry.
    if let Some(value) = unsafe { elements_pop_tail(elements) } {
        return Some(value);
    }
    Some(crate::array::js_array_pop_f64(elements))
}

/// The store's header when it is an ordinary, non-forwarded, unrestricted
/// Array — the only shape the lean append/pop below may touch.
///
/// # Safety
/// `elements` must be a live store address read from the meta slot.
#[inline]
unsafe fn plain_store_flags(elements: *mut ArrayHeader) -> Option<u16> {
    let header = crate::value::addr_class::try_read_gc_header(elements as usize)?;
    if crate::gc_type_is_not_array!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let blocking =
        crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
    (header._reserved & blocking == 0 && !super::array_has_element_descriptors(elements))
        .then_some(header._reserved)
}

/// Append without allocating: `Some(new_length)` when the store had spare
/// capacity, `None` when the caller must take the growing path.
///
/// # Safety
/// As [`plain_store_flags`].
#[inline]
unsafe fn elements_push_in_capacity(elements: *mut ArrayHeader, value: f64) -> Option<f64> {
    let flags = plain_store_flags(elements)?;
    let length = (*elements).length;
    let capacity = (*elements).capacity;
    if length >= capacity {
        return None;
    }
    crate::string::js_string_addref_if_heap_string(value);
    crate::array::store_array_slot_resolved(elements, length as usize, value, flags);
    (*elements).length = length + 1;
    crate::array::ek_note_published(elements, length as usize);
    Some(f64::from(length + 1))
}

/// Remove and return the tail element, or `None` when the complete runtime
/// entry has to run (empty store, a hole, an exotic flag).
///
/// # Safety
/// As [`plain_store_flags`].
#[inline]
unsafe fn elements_pop_tail(elements: *mut ArrayHeader) -> Option<f64> {
    plain_store_flags(elements)?;
    let length = (*elements).length;
    let capacity = (*elements).capacity;
    if length == 0 || length > capacity {
        return None;
    }
    let index = length - 1;
    let bits = slot_bits(elements, index);
    if bits == crate::value::TAG_HOLE {
        return None;
    }
    super::array_truncate_length(elements, index);
    Some(f64::from_bits(bits))
}

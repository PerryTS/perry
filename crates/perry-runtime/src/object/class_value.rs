//! A class CONSTRUCTOR used as a value (#11414; the every-receiver-shape
//! lane's class-constructor stage).
//!
//! Two forms name a class constructor while the migration runs:
//!
//! * the legacy INT32 immediate `0x7FFE_0000_0000_0000 | class_id` with bit 32
//!   clear (bit 32 set is the `C.prototype` half — [`super::class_prototype_ref_id`]).
//!   It is bit-identical to the int32 number equal to the class id, which is
//!   #11414; the lane deletes it;
//! * a class FUNCTION OBJECT: a `GC_TYPE_CLOSURE` cell whose code pointer is
//!   [`js_class_constructor_called`] (its [[Call]], which throws) and whose
//!   capture slot 0 holds the class id as an INT32 value.
//!
//! Every decoder asks [`class_value_id`] (or [`class_value_id_bits`] /
//! [`class_closure_id`] for the raw-word and raw-pointer spellings). Nothing
//! else may test the INT32 tag or the code pointer to decide "is this a class".
use crate::closure::ClosureHeader;

/// The class a constructor VALUE names — either form — or `None`. A
/// `C.prototype` reference is not a constructor and answers `None`.
#[inline]
pub(crate) fn class_value_id(value: f64) -> Option<u32> {
    class_value_id_bits(value.to_bits())
}

/// [`class_value_id`] on the NaN-boxed word.
#[inline]
pub(crate) fn class_value_id_bits(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0x7FFE => {
            if bits & super::native_module::CLASS_PROTOTYPE_REF_FLAG != 0 {
                return None;
            }
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            (class_id != 0 && super::is_class_id_registered(class_id)).then_some(class_id)
        }
        0x7FFD => class_closure_id((bits & crate::value::POINTER_MASK) as usize),
        _ => None,
    }
}

/// The class id of a class function object at raw address `ptr`, or `None`
/// for any other word. Ownership is proven (`is_closure_ptr`) before a header
/// byte is trusted, so arbitrary addresses are fine.
///
/// Callers include hot generic paths (bind, method values), so an ordinary
/// function is rejected before the ownership proof: once the address is a
/// plausible, aligned heap address whose ShapeId word is in the exotic band
/// (the same pre-checks `is_closure_ptr` makes before its first load), the
/// code-pointer word at +8 — inside every exotic cell's header — must be this
/// module's thunk. Only then is the cell proven.
#[inline]
pub fn class_closure_id(ptr: usize) -> Option<u32> {
    if !crate::value::addr_class::is_plausible_heap_addr(ptr)
        || !ptr.is_multiple_of(std::mem::align_of::<ClosureHeader>())
    {
        return None;
    }
    // SAFETY: a plausible, aligned heap address (the contract of the
    // `is_closure_ptr` pre-checks this mirrors).
    let shape =
        unsafe { *((ptr as *const u8).add(crate::closure::CLOSURE_SHAPE_OFFSET) as *const u32) };
    if !crate::object::shapes::is_exotic_shape_id(shape) {
        return None;
    }
    // SAFETY: an exotic-band ShapeId word means a closure-or-exotic header,
    // at least 16 bytes; +8 is the code pointer of a closure.
    let code = unsafe { *((ptr as *const u8).add(8) as *const *const u8) };
    if code != js_class_constructor_called as *const u8 || !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    // SAFETY: `is_closure_ptr` proved a live, non-forwarded closure cell.
    unsafe { class_closure_id_unchecked(ptr as *const ClosureHeader) }
}

/// [`class_closure_id`] for a cell already proven to be a live closure.
///
/// # Safety
/// `closure` is a live, non-forwarded `GC_TYPE_CLOSURE` cell.
#[inline]
pub(crate) unsafe fn class_closure_id_unchecked(closure: *const ClosureHeader) -> Option<u32> {
    if (*closure).func_ptr != js_class_constructor_called as *const u8 {
        return None;
    }
    let slot0 = *((closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const u64);
    Some((slot0 & 0xFFFF_FFFF) as u32)
}

/// [[Call]] of a class constructor: ES2015 9.2.1 step 2 — a class constructor
/// called without `new` throws a TypeError. It is the code pointer of every
/// class function object (and how one is recognized); [[Construct]] never
/// reaches it — `new` decodes the class id and runs the class's constructor.
#[no_mangle]
pub unsafe extern "C" fn js_class_constructor_called(closure: *const ClosureHeader) -> f64 {
    let name = unsafe { class_closure_id_unchecked(closure) }
        .and_then(super::class_registry::class_name_for_id)
        .unwrap_or_default();
    let message = format!("Class constructor {name} cannot be invoked without 'new'");
    crate::node_submodules::diagnostics::throw_type_error_no_code(message.as_bytes())
}

/// The PRE-MIGRATION gate spelling, kept exact for the legacy form while it
/// also admits the function-object form: any INT32 word (registered or not,
/// either half — those gates accepted every `0x7FFE` word, which is #11414)
/// or a class function object. `bits` is a NaN-boxed VALUE word. Every caller
/// is narrowed to [`class_value_id_bits`] when the INT32 form is deleted.
#[inline]
pub(crate) fn legacy_class_value_word(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0x7FFE => Some((bits & 0xFFFF_FFFF) as u32),
        0x7FFD => class_closure_id((bits & crate::value::POINTER_MASK) as usize),
        _ => None,
    }
}

/// [`legacy_class_value_word`] for a word that arrived through a POINTER-typed
/// parameter (`obj as u64`), which may be a raw untagged heap address.
#[inline]
pub(crate) fn legacy_class_ptr_word(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0 => class_closure_id(bits as usize),
        _ => legacy_class_value_word(bits),
    }
}

/// The NaN-boxed VALUE for a word [`legacy_class_ptr_word`] admitted: a raw
/// heap address gets its POINTER tag, anything else is already a value.
#[inline]
pub(crate) fn boxed_class_word(bits: u64) -> f64 {
    if bits >> 48 == 0 {
        f64::from_bits(crate::value::POINTER_TAG | bits)
    } else {
        f64::from_bits(bits)
    }
}

// ---------------------------------------------------------------------------
// The function object for each class: one per agent per class id.
// ---------------------------------------------------------------------------

/// Class ids per table page (log2). Class ids are dense per program plus a few
/// high reserved ids for built-in classes, so a two-level table keeps the
/// lookup a pair of indexed loads without a large flat array.
const CLASS_VALUE_PAGE_SHIFT: u32 = 8;
const CLASS_VALUE_PAGE_LEN: usize = 1 << CLASS_VALUE_PAGE_SHIFT;

type ClassValuePage = [*mut ClosureHeader; CLASS_VALUE_PAGE_LEN];

crate::perry_thread_local! {
    /// This agent's class function objects, indexed by class id: a page
    /// directory (`pages`, `len` pages) whose pages are leaked for the agent's
    /// life. Read without a borrow flag — the hot path is a TLS read, a bounds
    /// check and two loads. A GC root (rewritten on a move) via
    /// [`scan_class_value_roots_mut`].
    static CLASS_VALUES: std::cell::Cell<(*mut *mut ClassValuePage, usize)> =
        const { std::cell::Cell::new((std::ptr::null_mut(), 0)) };
}

#[inline]
fn class_value_cached(class_id: u32) -> Option<*mut ClosureHeader> {
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    let (pages, len) = CLASS_VALUES.with(std::cell::Cell::get);
    if page >= len {
        return None;
    }
    // SAFETY: `pages` holds `len` page pointers (null or a live leaked page).
    unsafe {
        let p = *pages.add(page);
        if p.is_null() {
            return None;
        }
        let c = (*p)[index];
        (!c.is_null()).then_some(c)
    }
}

/// The table slot for `class_id`, growing the directory / minting the page.
fn class_value_slot(class_id: u32) -> *mut *mut ClosureHeader {
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    let (mut pages, mut len) = CLASS_VALUES.with(std::cell::Cell::get);
    if page >= len {
        let new_len = (page + 1).next_power_of_two().max(4);
        let mut dir: Vec<*mut ClassValuePage> = vec![std::ptr::null_mut(); new_len];
        if !pages.is_null() {
            // SAFETY: the old directory holds `len` entries.
            unsafe { dir[..len].copy_from_slice(std::slice::from_raw_parts(pages, len)) };
            // The old directory is leaked: a concurrent reader on this agent
            // cannot exist (single-threaded agent), but the few bytes are not
            // worth a free/reuse protocol.
        }
        pages = Box::leak(dir.into_boxed_slice()).as_mut_ptr();
        len = new_len;
        CLASS_VALUES.with(|c| c.set((pages, len)));
    }
    // SAFETY: `page < len`.
    unsafe {
        let slot = pages.add(page);
        if (*slot).is_null() {
            *slot = Box::leak(Box::new([std::ptr::null_mut(); CLASS_VALUE_PAGE_LEN]));
        }
        (**slot).as_mut_ptr().add(index)
    }
}

/// Allocate the class function object for `class_id`: a closure born in the
/// old generation and pinned (it lives as long as the agent and never moves),
/// code pointer
/// [`js_class_constructor_called`], capture slot 0 = the class id as INT32.
///
/// Never collects: callers hold raw receiver pointers across the lookup, so
/// the old-arena allocation runs under a [`crate::gc::GcSuppressScope`].
#[cold]
#[inline(never)]
fn class_value_mint(class_id: u32) -> *mut ClosureHeader {
    let _no_collect = crate::gc::GcSuppressScope::new();
    let payload = crate::closure::closure_payload_size(1);
    let ptr = crate::arena::arena_alloc_gc_old_born_tenured(
        payload,
        std::mem::align_of::<ClosureHeader>(),
        crate::gc::GC_TYPE_CLOSURE,
    ) as *mut ClosureHeader;
    unsafe {
        // GC_STORE_AUDIT(INIT): fresh class function object; the one capture
        // is an INT32 class id and the props edge is null — pointer-free.
        (*ptr).capture_count = 1;
        (*ptr).shape_id = crate::closure::shape::function_dictionary_shape();
        (*ptr).func_ptr = js_class_constructor_called as *const u8;
        (*ptr).props = std::ptr::null_mut();
        std::ptr::write(
            crate::closure::closure_capture_slots_mut(ptr),
            crate::value::INT32_TAG | class_id as u64,
        );
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // Born old AND pinned: the address is the class's identity for the
        // agent's life (compiled code keeps it in registers and allocas, the
        // metadata and weak tables compare it), so no collector may move it.
        crate::gc::pin_user_ptr_non_young(ptr as *mut u8);
    }
    // SAFETY: the slot is this agent's table entry for `class_id`.
    unsafe { *class_value_slot(class_id) = ptr };
    crate::gc::runtime_write_barrier_root_heap_word(ptr as u64);
    ptr
}

/// The class function object for `class_id` on this agent (minted on first
/// use). `class_id` must be a registered class.
#[inline]
pub(crate) fn class_value_ptr(class_id: u32) -> *mut ClosureHeader {
    match class_value_cached(class_id) {
        Some(c) => c,
        None => class_value_mint(class_id),
    }
}

/// The VALUE of class `class_id`'s constructor: its function object, NaN-boxed.
#[inline]
pub(crate) fn class_value(class_id: u32) -> f64 {
    f64::from_bits(crate::value::POINTER_TAG | (class_value_ptr(class_id) as u64))
}

/// Emitted for every `Expr::ClassRef` and every place compiled code names a
/// class as a value (static `this`, `new.target`, `ns.C`): the class's
/// function object. A per-agent indexed load; never allocates after the
/// first use and never collects.
#[no_mangle]
pub extern "C" fn js_class_value(class_id: i32) -> f64 {
    class_value(class_id as u32)
}

/// GC root scan for [`CLASS_VALUES`]; registered in `gc::mod`'s runtime
/// scanner list.
pub(crate) fn scan_class_value_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let (pages, len) = CLASS_VALUES.with(std::cell::Cell::get);
    for i in 0..len {
        // SAFETY: `pages` holds `len` page pointers (null or a live page).
        let page = unsafe { *pages.add(i) };
        if page.is_null() {
            continue;
        }
        // SAFETY: a live leaked page of this agent.
        for slot in unsafe { (*page).iter_mut() } {
            if !slot.is_null() {
                visitor.visit_raw_mut_ptr_slot(slot);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Statics: the class function object's OWN properties.
// ---------------------------------------------------------------------------

/// A runtime-internal static key (private statics, computed-key records,
/// class captures): stored in the function object's internal state record,
/// never as a property.
#[inline]
fn is_internal_static_key(name: &str) -> bool {
    crate::object::is_internal_runtime_key(name)
}

/// Class `class_id`'s own static data property `name` (a declared static
/// field or a runtime `C.x = v`): a slot of its function object's own-property
/// bag.
pub(crate) fn class_static_get(class_id: u32, name: &str) -> Option<f64> {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: `class_value_ptr` returns this agent's live class closure.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_get(ptr, name)
        } else {
            crate::closure::props::bag_get(ptr, name.as_bytes())
        }
    }
}

/// Define/overwrite class `class_id`'s own static data property `name`.
pub(crate) fn class_static_set(class_id: u32, name: &str, value: f64) {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above; the bag writers run under a GcSuppressScope.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_set(ptr, name, value);
        } else {
            crate::closure::props::bag_set(ptr, name, value);
        }
    }
}

/// Remove class `class_id`'s own static data property `name`; true when it
/// existed.
pub(crate) fn class_static_remove(class_id: u32, name: &str) -> bool {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_remove(ptr, name)
        } else {
            crate::closure::props::bag_remove(ptr, name)
        }
    }
}

/// Class `class_id`'s own static data properties in own-key order (integer
/// keys ascending, then creation order). Internal keys are not properties and
/// never appear.
pub(crate) fn class_static_entries(class_id: u32) -> Vec<(String, f64)> {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above.
    unsafe { crate::closure::props::bag_snapshot(ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(cid: u32) {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }

    /// #11414: a class value is ONE function object per class — never an
    /// INT32 word a number can equal — pinned and old, so its address is its
    /// identity across collections.
    #[test]
    fn class_value_is_one_pinned_function_object_per_class() {
        let cid = 0x6A01;
        register(cid);
        let a = class_value(cid);
        let b = class_value(cid);
        assert_eq!(a.to_bits(), b.to_bits(), "one function object per class");
        let v = crate::value::JSValue::from_bits(a.to_bits());
        assert!(
            !v.is_int32() && !v.is_number(),
            "a class value is not a number"
        );
        assert!(v.is_pointer());
        let ptr = (a.to_bits() & crate::value::POINTER_MASK) as usize;
        assert!(
            crate::closure::is_closure_ptr(ptr),
            "a GC_TYPE_CLOSURE cell"
        );
        assert_eq!(class_value_id(a), Some(cid));
        assert_eq!(class_closure_id(ptr), Some(cid));
        // The number equal to the class id is not the class.
        assert_ne!(
            f64::from_bits(crate::value::INT32_TAG | cid as u64).to_bits(),
            a.to_bits()
        );
        let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr) }.expect("header");
        assert_ne!(header.gc_flags & crate::gc::GC_FLAG_PINNED, 0, "pinned");
        assert_ne!(header.gc_flags & crate::gc::GC_FLAG_TENURED, 0, "born old");
        crate::gc::js_gc_collect();
        assert_eq!(class_value(cid).to_bits(), a.to_bits(), "never moves");
        assert_eq!(class_value_id(a), Some(cid), "survives a full collection");
        let other = class_value(0x6A02);
        assert_ne!(other.to_bits(), a.to_bits());
        assert_eq!(class_value_id(other), Some(0x6A02));
    }

    /// The table is a root: the scan visits every minted class value.
    #[test]
    fn class_value_table_is_scanned() {
        let cid = 0x6B01;
        register(cid);
        let ptr = class_value_ptr(cid) as usize;
        let mut seen = false;
        scan_class_value_roots_mut(&mut crate::gc::RuntimeRootVisitor::for_copy(
            &mut |v: f64| {
                let bits = v.to_bits();
                if bits as usize == ptr || (bits & crate::value::POINTER_MASK) as usize == ptr {
                    seen = true;
                }
            },
        ));
        assert!(seen, "the class-value table must be a GC root");
    }

    /// Only the function object's own code pointer names a class.
    #[test]
    fn ordinary_closures_and_numbers_are_not_class_values() {
        extern "C" fn body() {}
        let c = crate::closure::js_closure_alloc(body as *const u8, 0);
        assert_eq!(class_closure_id(c as usize), None);
        assert_eq!(class_value_id(42.0), None);
        assert_eq!(
            class_value_id(f64::from_bits(crate::value::TAG_UNDEFINED)),
            None
        );
    }
}

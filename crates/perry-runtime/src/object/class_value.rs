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
#[inline]
pub fn class_closure_id(ptr: usize) -> Option<u32> {
    if !crate::closure::is_closure_ptr(ptr) {
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

type ClassValuePage = Box<[*mut ClosureHeader; CLASS_VALUE_PAGE_LEN]>;

crate::perry_thread_local! {
    /// This agent's class function objects, indexed by class id. A GC root
    /// (rewritten on a move) via [`scan_class_value_roots_mut`].
    static CLASS_VALUES: std::cell::RefCell<Vec<Option<ClassValuePage>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn class_value_cached(class_id: u32) -> Option<*mut ClosureHeader> {
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    CLASS_VALUES.with(|t| {
        let t = t.borrow();
        let p = t.get(page)?.as_ref()?;
        let c = p[index];
        (!c.is_null()).then_some(c)
    })
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
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    CLASS_VALUES.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() <= page {
            t.resize_with(page + 1, || None);
        }
        t[page].get_or_insert_with(|| Box::new([std::ptr::null_mut(); CLASS_VALUE_PAGE_LEN]))
            [index] = ptr;
    });
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
    CLASS_VALUES.with(|t| {
        let mut t = t.borrow_mut();
        for page in t.iter_mut().flatten() {
            for slot in page.iter_mut() {
                if !slot.is_null() {
                    visitor.visit_raw_mut_ptr_slot(slot);
                }
            }
        }
    });
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

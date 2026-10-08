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
//!   [`js_class_constructor_called`] (its [[Call]], which throws), whose
//!   capture slot 0 holds the class id as an INT32 value, and whose capture
//!   slot 1 holds the class's `prototype` object ([`class_decl_prototype_link`]).
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
    // at least 16 bytes; +8 is the info word of a closure (compared, never
    // dereferenced here).
    let info =
        unsafe { *((ptr as *const u8).add(8) as *const *const crate::closure::JsFunctionInfo) };
    if !std::ptr::eq(info, &CLASS_CONSTRUCTOR_INFO) {
        return None;
    }
    class_closure_id_exotic(ptr)
}

/// [`class_closure_id`] past its inline pre-filter (a plausible, aligned heap
/// address whose ShapeId word is in the exotic band): out of line, so the
/// many gates that inline the pre-filter stay small.
#[inline(never)]
fn class_closure_id_exotic(ptr: usize) -> Option<u32> {
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
    if !std::ptr::eq((*closure).info, &CLASS_CONSTRUCTOR_INFO) {
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
pub unsafe extern "C" fn js_class_constructor_called(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let name = unsafe { class_closure_id_unchecked(closure) }
        .and_then(super::class_registry::class_name_for_id)
        .unwrap_or_default();
    let message = format!("Class constructor {name} cannot be invoked without 'new'");
    crate::node_submodules::diagnostics::throw_type_error_no_code(message.as_bytes())
}

/// The one info of every class function object: its body is
/// [`js_class_constructor_called`]. A class function object is recognized by
/// its info word being exactly this static (one pointer compare, no deref).
pub(crate) static CLASS_CONSTRUCTOR_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        js_class_constructor_called as crate::codegen_abi::JsBody0<ClosureHeader>,
    );

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

/// Class ids per table page (log2). Within each class-id band (see
/// [`class_value_band`]) ids are dense from the band's base, so a two-level
/// table keeps the lookup a pair of indexed loads without a large flat array.
const CLASS_VALUE_PAGE_SHIFT: u32 = 8;
const CLASS_VALUE_PAGE_LEN: usize = 1 << CLASS_VALUE_PAGE_SHIFT;

type ClassValuePage = [*mut ClosureHeader; CLASS_VALUE_PAGE_LEN];

/// One band's page directory: `len` page pointers (null or a leaked page).
#[repr(C)]
#[derive(Clone, Copy)]
struct ClassValueDir {
    pages: *mut *mut ClassValuePage,
    len: usize,
}

/// The class-id bands, each with its own page directory. A class id is never
/// an index by itself: it is an OFFSET from its band's base, so a directory is
/// sized by how many ids of that band exist, never by the id's magnitude.
/// (Indexing by the raw id made the first `0xFFFF_xxxx` builtin — fs.Stats,
/// `0xFFFF_0070` — grow one directory to 16M pages: 128 MiB zero-filled.)
///
/// * band 0: compiled classes, `1..0x7FFF_FF00` — dense from 1 per program;
/// * band 1: the runtime builtin band `0x7FFF_FF00..=0x7FFF_FFFF` (256 ids);
/// * band 2: synthetic ids (`class X extends f`'s parent), a counter from
///   [`SYNTHETIC_CLASS_ID_BASE`];
/// * band 3: the reserved native-class band `0xFFFF_0000..` (at most 65,536
///   ids, so at most 256 pages and a 2 KiB directory).
///
/// Anything else (`0`, the ShapeId range) is not a class id and has no slot.
const CLASS_VALUE_BANDS: usize = 4;
const FIRST_RUNTIME_CLASS_ID: u32 = 0x7FFF_FF00;
const SYNTHETIC_CLASS_ID_BASE: u32 =
    super::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE;
const SYNTHETIC_CLASS_ID_END: u32 =
    super::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_END;

/// The band and in-band offset of `class_id`, or `None` for a word that is
/// not a class id. The one place a class id becomes a table position (the
/// compiled band's offset is the id itself, which is what
/// [`class_value_cached`]'s fast path reads).
fn class_value_band(class_id: u32) -> Option<(usize, u32)> {
    match class_id {
        0 => None,
        1..FIRST_RUNTIME_CLASS_ID => Some((0, class_id)),
        FIRST_RUNTIME_CLASS_ID..=0x7FFF_FFFF => Some((1, class_id - FIRST_RUNTIME_CLASS_ID)),
        SYNTHETIC_CLASS_ID_BASE..SYNTHETIC_CLASS_ID_END => {
            Some((2, class_id - SYNTHETIC_CLASS_ID_BASE))
        }
        SYNTHETIC_CLASS_ID_END..=u32::MAX => Some((3, class_id - SYNTHETIC_CLASS_ID_END)),
        _ => None,
    }
}

#[allow(clippy::declare_interior_mutable_const)]
const EMPTY_CLASS_VALUE_DIR: std::cell::Cell<ClassValueDir> = std::cell::Cell::new(ClassValueDir {
    pages: std::ptr::null_mut(),
    len: 0,
});

crate::perry_thread_local! {
    /// This agent's class function objects: per class-id band
    /// ([`class_value_band`]) a page directory whose pages are leaked for the
    /// agent's life. Read without a borrow flag — the hot path is a TLS read,
    /// a bounds check and two loads. Not a GC root: a class function object
    /// is pinned (never moves, always a root of the pinned-object scan) and
    /// its edges (`props`, the prototype link) are traced child slots like any
    /// other closure's.
    static CLASS_VALUES: [std::cell::Cell<ClassValueDir>; CLASS_VALUE_BANDS] =
        const { [EMPTY_CLASS_VALUE_DIR; CLASS_VALUE_BANDS] };
}

/// Band `band`'s directory.
#[inline(always)]
fn class_value_dir(band: usize) -> (*mut *mut ClassValuePage, usize) {
    CLASS_VALUES.with(|bands| {
        let d = bands[band].get();
        (d.pages, d.len)
    })
}

/// Every band's directory (tests and resets).
#[cfg(test)]
fn class_value_dirs() -> [(*mut *mut ClassValuePage, usize); CLASS_VALUE_BANDS] {
    std::array::from_fn(class_value_dir)
}

#[inline]
fn class_value_cached(class_id: u32) -> Option<*mut ClosureHeader> {
    // The compiled band's offset IS the class id, and its directory never
    // grows past `BAND0_MAX_PAGES`, so every id of a higher band misses this
    // bounds check: the hot path is the plain two-load lookup, and only a
    // miss asks which band the id belongs to.
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let (pages, len) = class_value_dir(0);
    if page < len {
        // SAFETY: `pages` holds `len` page pointers (null or a live page).
        return unsafe { class_value_page_get(pages, page, class_id) };
    }
    if class_id < FIRST_RUNTIME_CLASS_ID {
        return None;
    }
    class_value_cached_high(class_id)
}

/// [`class_value_cached`] for an id above the compiled band.
#[inline(never)]
fn class_value_cached_high(class_id: u32) -> Option<*mut ClosureHeader> {
    let (band, offset) = class_value_band(class_id)?;
    let page = (offset >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let (pages, len) = class_value_dir(band);
    if page >= len {
        return None;
    }
    // SAFETY: `pages` holds `len` page pointers (null or a live page).
    unsafe { class_value_page_get(pages, page, offset) }
}

/// Entry `offset` of directory page `page`.
///
/// # Safety
/// `page` < the directory's length; `pages` holds that many page pointers
/// (null or a live leaked page).
#[inline(always)]
unsafe fn class_value_page_get(
    pages: *mut *mut ClassValuePage,
    page: usize,
    offset: u32,
) -> Option<*mut ClosureHeader> {
    let p = *pages.add(page);
    if p.is_null() {
        return None;
    }
    let c = (*p)[offset as usize & (CLASS_VALUE_PAGE_LEN - 1)];
    (!c.is_null()).then_some(c)
}

/// The compiled band's directory bound: one page short of the first page a
/// runtime-band id would index, so [`class_value_cached`]'s fast bounds
/// check rejects every id of a higher band.
const BAND0_MAX_PAGES: usize = (FIRST_RUNTIME_CLASS_ID >> CLASS_VALUE_PAGE_SHIFT) as usize;

/// The table slot for `class_id`, growing its band's directory / minting the
/// page. `class_id` is a class id ([`class_value_band`] answers).
fn class_value_slot(class_id: u32) -> *mut *mut ClosureHeader {
    let Some((band, offset)) = class_value_band(class_id) else {
        panic!("class_value_slot: {class_id:#x} is not a class id");
    };
    let page = (offset >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = offset as usize & (CLASS_VALUE_PAGE_LEN - 1);
    let (mut pages, mut len) = class_value_dir(band);
    if page >= len {
        let mut new_len = (page + 1).next_power_of_two().max(4);
        if band == 0 {
            // `page` < BAND0_MAX_PAGES: a band-0 offset is below the first
            // runtime id.
            new_len = new_len.min(BAND0_MAX_PAGES);
        }
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
        CLASS_VALUES.with(|bands| bands[band].set(ClassValueDir { pages, len }));
    }
    perry_class_value_dir_cell();
    // SAFETY: `page < len`.
    unsafe {
        let slot = pages.add(page);
        if (*slot).is_null() {
            *slot = Box::leak(Box::new([std::ptr::null_mut(); CLASS_VALUE_PAGE_LEN]));
        }
        (**slot).as_mut_ptr().add(index)
    }
}

/// Capture slot of a class function object holding its evaluation state, an
/// INT32. `0`: the object names the class's evaluations as a group, so a
/// per-evaluation class object's static writes are mirrored into it (#6530).
/// `1`: the object IS its declaration's first evaluation (#11759 (c′)), a
/// class of its own; later evaluations never write into it. Generated code
/// sets it when the first evaluation hands the shared class out.
const CLASS_EVALUATION_STATE_SLOT: usize = crate::codegen_abi::CLASS_EVALUATION_STATE_CAPTURE;

/// Is class `class_id`'s function object its declaration's first evaluation
/// (see [`CLASS_EVALUATION_STATE_SLOT`])? A class whose function object was
/// never minted has no evaluation to protect.
pub(crate) fn class_value_is_first_evaluation(class_id: u32) -> bool {
    class_value_cached(class_id).is_some_and(|closure| {
        // SAFETY: a live class function object minted with its capture slots.
        unsafe {
            *crate::closure::closure_capture_slots_mut(closure).add(CLASS_EVALUATION_STATE_SLOT)
                == crate::codegen_abi::CLASS_FIRST_EVALUATION_STATE
        }
    })
}

/// Allocate the class function object for `class_id`: a closure born in the
/// old generation and pinned (it lives as long as the agent and never moves),
/// code pointer
/// [`js_class_constructor_called`], capture slot 0 = the class id as INT32,
/// capture slot 1 = its evaluation state ([`CLASS_EVALUATION_STATE_SLOT`]),
/// capture slot 2 = the class's `prototype` object once it exists.
///
/// Never collects: callers hold raw receiver pointers across the lookup, so
/// the old-arena allocation runs under a [`crate::gc::GcSuppressScope`].
#[cold]
#[inline(never)]
fn class_value_mint(class_id: u32) -> *mut ClosureHeader {
    // Any class id may own one: a native class whose prototype carries
    // registered members (fs.Stats' Date getters) keeps its prototype link
    // here like a compiled class. Its band, not its magnitude, places it.
    debug_assert!(
        class_value_band(class_id).is_some(),
        "a class function object belongs to a class id: {class_id:#x}"
    );
    let _no_collect = crate::gc::GcSuppressScope::new();
    let payload = crate::closure::closure_payload_size(CLASS_VALUE_CAPTURES);
    let ptr = crate::arena::arena_alloc_gc_old_born_tenured(
        payload,
        std::mem::align_of::<ClosureHeader>(),
        crate::gc::GC_TYPE_CLOSURE,
    ) as *mut ClosureHeader;
    unsafe {
        (*ptr).capture_count = CLASS_VALUE_CAPTURES as u32;
        (*ptr).shape_id = crate::closure::shape::function_class_shape();
        (*ptr).info = &CLASS_CONSTRUCTOR_INFO;
        (*ptr).props = std::ptr::null_mut();
        let captures = crate::closure::closure_capture_slots_mut(ptr);
        // Captures 0 and 1 are INT32s (class id, evaluation state) and capture
        // 2, the prototype link, starts `undefined`. The arena birth leaves
        // the layout UNKNOWN, not pointer-free, so the tracer reads capture 2
        // once `class_decl_prototype_link_store` installs a pointer there
        // through the slot barrier.
        // GC_STORE_AUDIT(INIT): fresh class function object; no pointer yet.
        std::ptr::write(captures, crate::value::INT32_TAG | class_id as u64);
        std::ptr::write(
            captures.add(CLASS_PROTOTYPE_LINK_CAPTURE),
            crate::value::TAG_UNDEFINED,
        );
        std::ptr::write(
            crate::closure::closure_capture_slots_mut(ptr).add(CLASS_EVALUATION_STATE_SLOT),
            crate::value::INT32_TAG,
        );
        // Born old AND pinned: the address is the class's identity for the
        // agent's life (compiled code keeps it in registers and allocas, the
        // metadata and weak tables compare it), so no collector may move it.
        crate::gc::pin_user_ptr_non_young(ptr as *mut u8);
    }
    // SAFETY: the slot is this agent's table entry for `class_id`.
    unsafe { *class_value_slot(class_id) = ptr };
    crate::gc::runtime_write_barrier_root_heap_word(ptr as u64);
    // Still inside the no-collect scope: the own-property object and its
    // keys allocate.
    for key in INTRINSIC_OWN_DATA_KEYS {
        install_intrinsic_own_data(class_id, key);
    }
    // MakeConstructor: `prototype` { !w, !e, !c } is created with the class,
    // after `length` and `name` and before every ClassBody static, so the own
    // key order is the bag's creation order.
    let proto = super::class_registry::class_decl_prototype_value(class_id);
    if crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
        // SAFETY: `ptr` is the class closure minted above.
        unsafe { crate::closure::props::bag_define_value(ptr as usize, "prototype", proto) };
        super::class_registry::class_static_set_defined_attrs(
            class_id,
            "prototype",
            false,
            false,
            false,
        );
    }
    // ClassBody static methods and accessors, in ClassBody order.
    for key in super::class_registry::class_own_string_member_names(class_id, true) {
        install_declared_static_accessor(class_id, &key);
        install_declared_static_method(class_id, &key);
    }
    ptr
}

/// A class constructor's `length` and `name`, in creation order
/// (ClassDefinitionEvaluation: SetFunctionLength, then SetFunctionName).
const INTRINSIC_OWN_DATA_KEYS: [&str; 2] = ["length", "name"];

/// The attributes of a function's own `length` / `name`.
pub(crate) const INTRINSIC_ATTRS: (bool, bool, bool) = (false, false, true);

/// The value of intrinsic own data property `key` of class `class_id`, if
/// the class registered one.
pub(crate) fn intrinsic_own_data_value(class_id: u32, key: &str) -> Option<f64> {
    match key {
        "length" => super::class_registry::class_length_for_id(class_id).map(f64::from),
        "name" => super::class_registry::class_name_for_id(class_id).map(|name| {
            let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
        }),
        _ => None,
    }
}

/// Does a static method or accessor of class `class_id` own `key`? Then it,
/// not the intrinsic data property, is the class's own `key`.
pub(crate) fn static_member_owns(class_id: u32, key: &str) -> bool {
    super::class_registry::class_has_own_static_method(class_id, key)
        || super::class_registry::class_registered_static_accessor_ptrs(class_id, key).is_some()
}

/// Is own `key` of class `class_id` still the intrinsic data property (not
/// replaced by a static field, a `defineProperty`, or deleted)?
fn holds_intrinsic(class_id: u32, key: &str) -> bool {
    class_static_get(class_id, key).is_some()
        && super::class_registry::class_static_defined_attrs(class_id, key) == Some(INTRINSIC_ATTRS)
}

/// ClassDefinitionEvaluation's SetFunctionLength / SetFunctionName: `length`
/// and `name` are own DATA properties of the class's function object,
/// `{ writable: false, enumerable: false, configurable: true }`, kept in its
/// own-property object with every other own data property — so `C.name` and
/// `x.constructor.name` are one lookup in that object's shape. A static
/// method or accessor of the same name is the class's own property instead,
/// and a static field or `defineProperty` of that name replaces it.
fn install_intrinsic_own_data(class_id: u32, key: &str) {
    if static_member_owns(class_id, key) {
        return;
    }
    let Some(value) = intrinsic_own_data_value(class_id, key) else {
        return;
    };
    class_static_set(class_id, key, value);
    let (writable, enumerable, configurable) = INTRINSIC_ATTRS;
    super::class_registry::class_static_set_defined_attrs(
        class_id,
        key,
        writable,
        enumerable,
        configurable,
    );
}

/// A static field `key` was defined on class `class_id`: if it replaced the
/// intrinsic `name` / `length`, the property keeps the field's (ordinary)
/// attributes, not the intrinsic's.
pub(crate) fn note_static_field_defined(class_id: u32, key: &str) {
    if INTRINSIC_OWN_DATA_KEYS.contains(&key)
        && super::class_registry::class_static_defined_attrs(class_id, key) == Some(INTRINSIC_ATTRS)
    {
        super::class_registry::class_static_clear_defined_attrs(class_id, key);
    }
}

/// The registry changed what class `class_id`'s intrinsic `key` is (its
/// name or length registered, or a static method / accessor of that name
/// registered) after this agent minted its function object: bring the own
/// property in line. A key the program already redefined or deleted is left
/// alone.
pub(crate) fn note_intrinsic_registration(class_id: u32, key: &str) {
    if class_value_cached(class_id).is_none() {
        return;
    }
    let _no_collect = crate::gc::GcSuppressScope::new();
    // A ClassBody static accessor registered after the object exists (a
    // computed key registers when the class definition evaluates).
    if super::class_registry::class_registered_static_accessor_ptrs(class_id, key).is_some() {
        install_declared_static_accessor(class_id, key);
    }
    if INTRINSIC_OWN_DATA_KEYS.contains(&key) {
        note_intrinsic_key_registration(class_id, key);
    }
    install_declared_static_method(class_id, key);
}

fn note_intrinsic_key_registration(class_id: u32, key: &str) {
    if holds_intrinsic(class_id, key) {
        if static_member_owns(class_id, key) {
            class_static_remove(class_id, key);
            super::class_registry::class_static_clear_defined_attrs(class_id, key);
        } else if let Some(value) = intrinsic_own_data_value(class_id, key) {
            class_static_set(class_id, key, value);
        }
    } else if class_static_get(class_id, key).is_none()
        // SAFETY: minted above (`class_value_cached`).
        && !unsafe { crate::closure::props::state_is_deleted(class_value_ptr(class_id) as usize, key) }
    {
        install_intrinsic_own_data(class_id, key);
    }
}

/// `delete C.<name>` removed own `name` of class `class_id`: an intrinsic
/// `name` / `length` is remembered on the object (as for any function, #3655),
/// so a later registration does not install it again.
pub(crate) fn note_static_key_deleted(class_id: u32, name: &str) {
    if INTRINSIC_OWN_DATA_KEYS.contains(&name) {
        // SAFETY: this agent's live class closure.
        unsafe {
            crate::closure::props::state_mark_deleted(class_value_ptr(class_id) as usize, name)
        };
    }
}

/// Has `delete` removed class `class_id`'s own static member `name` — a
/// ClassBody static method or accessor, or the intrinsic `name` / `length`?
/// Derived from the function object: the member was declared and the object
/// no longer has the key. A never-minted object has deleted nothing.
pub(crate) fn class_static_key_deleted(class_id: u32, name: &str) -> bool {
    if name.starts_with('#') || is_internal_static_key(name) {
        return false;
    }
    let Some(ptr) = class_value_if_minted(class_id) else {
        return false;
    };
    let declared = static_member_owns(class_id, name)
        || (INTRINSIC_OWN_DATA_KEYS.contains(&name)
            && intrinsic_own_data_registered(class_id, name));
    // SAFETY: this agent's live class closure; the lookup does not allocate.
    declared && !unsafe { crate::closure::props::bag_has_own(ptr as usize, name.as_bytes()) }
}

fn intrinsic_own_data_registered(class_id: u32, key: &str) -> bool {
    match key {
        "length" => super::class_registry::class_length_for_id(class_id).is_some(),
        "name" => super::class_registry::class_name_for_id(class_id).is_some(),
        _ => false,
    }
}

/// What own property `name` of class `class_id`'s function object says about
/// the ClassBody static method `name` whose code is `func_ptr`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StaticMethodProperty {
    /// The property is still the declaration's function: dispatch it.
    Live,
    /// `delete` removed it: the lookup continues at the parent class.
    Deleted,
    /// The program redefined it: the property's value wins.
    Replaced,
}

/// Class `class_id`'s answer for static method `name` during a chain walk.
/// `declared` is its own ClassBody declaration of `name` (the declaration's
/// closure-convention entry, 0 when it has none), `None` when it declares no
/// such method.
pub(crate) fn static_method_property(
    class_id: u32,
    name: &str,
    declared: Option<usize>,
) -> StaticMethodProperty {
    let live_or_next = if declared.is_some() {
        StaticMethodProperty::Live
    } else {
        StaticMethodProperty::Deleted
    };
    if name.starts_with('#') || is_internal_static_key(name) {
        return live_or_next;
    }
    // A template evaluated to class objects keeps its statics on those objects
    // (`define_class_object_own_properties`), each evaluation its own. The
    // shared function object minted for the template is only the last-wins
    // mirror of writes to any of them (`mirror_class_object_static_write`): it
    // says nothing about what the declaration is, and one evaluation's
    // `C.m = f` must not retire `m` for its siblings.
    if super::class_registry::template_has_class_objects(class_id) {
        return live_or_next;
    }
    // A never-minted object owns exactly its declarations.
    let Some(ptr) = class_value_if_minted(class_id) else {
        return live_or_next;
    };
    let code = declared.unwrap_or(0);
    // SAFETY: this agent's live class closure; nothing below allocates.
    unsafe {
        match crate::closure::props::bag_get(ptr as usize, name.as_bytes()) {
            Some(v) if code != 0 && static_method_code_of(v) == Some(code) => {
                StaticMethodProperty::Live
            }
            Some(_) => StaticMethodProperty::Replaced,
            None if crate::closure::props::bag_has_own(ptr as usize, name.as_bytes()) => {
                StaticMethodProperty::Replaced
            }
            // A declaration without an entry is never installed as a property.
            None if declared == Some(0) => StaticMethodProperty::Live,
            None => StaticMethodProperty::Deleted,
        }
    }
}

/// The `JsFunctionInfo` of `value` when it is a function object (a closure): the
/// identity of the body it runs.
unsafe fn static_method_code_of(value: f64) -> Option<usize> {
    let js = crate::JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let f = js.as_pointer::<ClosureHeader>();
    if !crate::closure::is_closure_ptr(f as usize) {
        return None;
    }
    Some((*f).info as usize)
}

/// The class whose ClassBody static method `name` a read of `name` on class
/// `class_id` finds, when the property found is still that declaration's
/// function object: its value, minting the owner's function object (so every
/// subclass reads the one own property, `Q.a === P.a`).
pub(crate) fn inherited_static_method_value(class_id: u32, name: &str) -> Option<f64> {
    let (owner, _) = super::class_registry::lookup_static_method_owner(class_id, name)?;
    super::class_registry::class_own_static_method_code(owner, name)?;
    class_value(owner);
    class_static_get(owner, name)
}

/// Does own `key` of the class function object `ptr` hold its ClassBody
/// static method's function object (the declaration's code)? That fact is
/// part of the object's shape: a write that ends it transitions the shape
/// (`object::class_value::js_class_static_call_guard`).
pub(crate) unsafe fn holds_declared_static_method(ptr: usize, key: &str) -> Option<f64> {
    let cid = class_closure_id_unchecked(ptr as *const ClosureHeader)?;
    let code = super::class_registry::class_own_static_method_code(cid, key)?;
    let v = crate::closure::props::bag_get(ptr, key.as_bytes())?;
    let js = crate::JSValue::from_bits(v.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let f = js.as_pointer::<ClosureHeader>();
    (crate::closure::is_closure_ptr(f as usize) && (*f).info as usize == code).then_some(v)
}

/// [`js_class_static_call_guard`] for a call whose receiver is a value
/// (`(C as any).m()`, a local holding the class): the receiver must be its
/// class's function object on this agent, or the call reads the property.
///
/// # Safety
/// As [`js_class_static_call_guard`].
#[no_mangle]
pub unsafe extern "C" fn js_class_static_value_call_guard(
    receiver: f64,
    owner_id: i32,
    name_ptr: *const u8,
    name_len: i64,
    body: i64,
    memo: *mut u64,
) -> i32 {
    let bits = receiver.to_bits();
    let Some(cid) = class_value_id_bits(bits) else {
        return 0;
    };
    if legacy_class_value_word(bits).is_none() {
        let js = crate::JSValue::from_bits(bits);
        if !js.is_pointer()
            || class_value_cached(cid).map(|c| c as usize) != Some(js.as_pointer::<u8>() as usize)
        {
            return 0;
        }
    }
    js_class_static_call_guard(cid as i32, owner_id, name_ptr, name_len, body, memo)
}

/// The shape code of class `class_id`'s function object for a static-call
/// memo: 0 when this agent never minted it (it owns exactly its
/// declarations), `u32::MAX - 1` when it has no own-property bag, else the
/// bag's ShapeId.
fn static_call_shape_code(class_id: u32) -> u32 {
    match class_value_cached(class_id) {
        None => 0,
        // SAFETY: this agent's live class closure; a shape load.
        Some(c) => unsafe {
            let bag = crate::closure::props::bag_of(c as usize);
            if bag.is_null() {
                u32::MAX - 1
            } else {
                super::shapes::object_shape_stamp(bag)
            }
        },
    }
}

/// Codegen's direct static call `C.m(..)` runs the declared body `body` only
/// while the property the call reads — own `m` of C, or of the class `owner`
/// it inherits from — is still that declaration's function object. The fact
/// lives in the class function objects' shapes: installing a declaration,
/// storing a different value over it, and deleting it each transition the
/// shape (`closure::props`), so an unchanged pair of shapes proves it.
///
/// `memo` is the site's [`StaticCallMemo`]. The site's hit is inline in
/// compiled code: it loads the two function objects' shape words through the
/// memo's pointers and compares them with `key`; only a miss calls this,
/// which re-validates by reading the property and re-arms the memo (only for
/// a one-link chain, whose two shapes cover every object the read consults).
///
/// # Safety
/// `name_ptr` points at `name_len` bytes; `memo` is null or the site's
/// [`StaticCallMemo`] (thread-local when the program starts workers, so the
/// function objects it names are this agent's).
#[no_mangle]
pub unsafe extern "C" fn js_class_static_call_guard(
    class_id: i32,
    owner_id: i32,
    name_ptr: *const u8,
    name_len: i64,
    body: i64,
    memo: *mut u64,
) -> i32 {
    let cid = class_id as u32;
    let owner = owner_id as u32;
    // SAFETY: null or the site's memo (see `# Safety`).
    let memo = (!memo.is_null()).then(|| &mut *(memo as *mut StaticCallMemo));
    if name_ptr.is_null() || name_len <= 0 {
        return 1;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize))
    else {
        return 1;
    };
    match super::class_registry::lookup_static_method_owner(cid, name) {
        Some((found, (func_ptr, ..))) if func_ptr == body as usize => {
            let one_link =
                found == cid || super::class_registry::get_parent_class_id(cid) == Some(found);
            if let Some(m) = memo {
                if owner != 0 && found == owner && one_link {
                    arm_static_call_memo(m, cid, owner);
                }
            }
            1
        }
        _ => 0,
    }
}

/// A static-call site's memo (`perry-codegen/src/expr/static_method.rs`),
/// four words the emitted hit reads (`perry_abi::STATIC_CALL_MEMO_*`):
/// `key` = (C's shape word | owner's shape word << 32) of the last
/// validation; the two class function objects whose own-property objects'
/// shape words the hit loads (pinned for the agent's life, so the pointers
/// never go stale); and C's function object as a value, for a site whose
/// receiver is a value.
///
/// Never armed, `c` and `owner` point at a constant of the site's whose
/// own-property word points at itself, a shape word of 0, and `key` is all
/// ones, so the hit needs no "armed?" test: that shape word never equals a
/// half of the unarmed key.
#[repr(C)]
pub struct StaticCallMemo {
    pub key: u64,
    pub c: usize,
    pub owner: usize,
    pub c_value: u64,
}

const _: () = {
    assert!(
        std::mem::offset_of!(StaticCallMemo, key)
            == crate::codegen_abi::STATIC_CALL_MEMO_KEY_OFFSET
    );
    #[cfg(target_pointer_width = "64")]
    {
        assert!(
            std::mem::offset_of!(StaticCallMemo, c)
                == crate::codegen_abi::STATIC_CALL_MEMO_C_OFFSET
        );
        assert!(
            std::mem::offset_of!(StaticCallMemo, owner)
                == crate::codegen_abi::STATIC_CALL_MEMO_OWNER_OFFSET
        );
        assert!(
            std::mem::offset_of!(StaticCallMemo, c_value)
                == crate::codegen_abi::STATIC_CALL_MEMO_VALUE_OFFSET
        );
    }
};

/// Arm `memo` for the one-link chain (`cid`, `owner`) just validated. The
/// inline hit compares raw shape words, so it is armed only when both are
/// ShapeIds (a raw word equals a ShapeId only when it is that ShapeId); the
/// function objects are minted here (an unobservable act: a fresh object owns
/// exactly its declarations), so a class used only through static calls
/// still gets the inline hit.
fn arm_static_call_memo(memo: &mut StaticCallMemo, cid: u32, owner: u32) {
    let c = class_value_ptr(cid);
    let o = class_value_ptr(owner);
    let (sc, so) = (static_call_shape_code(cid), static_call_shape_code(owner));
    if !super::shapes::is_shape_id(sc) || !super::shapes::is_shape_id(so) {
        return;
    }
    // GC_STORE_AUDIT(ROOT): a compiled site's memo naming PINNED class
    // function objects (never move), also rooted by the class-value table.
    memo.c = c as usize;
    memo.owner = o as usize;
    memo.c_value = crate::value::POINTER_TAG | (c as u64);
    memo.key = u64::from(sc) | u64::from(so) << 32;
}

/// Does class `class_id`'s function object own static method `name` as a data
/// property it has not deleted (the declaration, or a value put in its place)?
pub(crate) fn class_static_owns_method(class_id: u32, name: &str) -> bool {
    if class_static_get(class_id, name).is_some() {
        return true;
    }
    super::class_registry::class_own_static_method_entry(class_id, name).is_some()
        && class_value_if_minted(class_id).is_none_or(|_| {
            super::class_registry::class_own_static_method_code(class_id, name).is_none()
        })
}

/// ClassDefinitionEvaluation's static methods: ClassBody static method
/// `name` of class `class_id` is an own data property of its function object,
/// `{ writable: true, enumerable: false, configurable: true }`, whose value is
/// one function object per (class, method) running exactly this
/// declaration's code (a resolved entry, never a by-name dispatch). A
/// property the program redefined is left alone; a re-registered entry
/// (a class expression evaluated again) refreshes the function.
fn install_declared_static_method(class_id: u32, name: &str) {
    if name.starts_with('#') || is_internal_static_key(name) {
        return;
    }
    let Some(code) = super::class_registry::class_own_static_method_code(class_id, name) else {
        return;
    };
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: this agent's live class closure.
    unsafe {
        match crate::closure::props::bag_get(ptr, name.as_bytes()) {
            // Already this declaration's function, or a value the program
            // put in its place: leave it.
            Some(_) => return,
            None if crate::closure::props::bag_has_own(ptr, name.as_bytes())
                || crate::closure::props::state_is_deleted(ptr, name) =>
            {
                return
            }
            None => {}
        }
    }
    let _no_collect = crate::gc::GcSuppressScope::new();
    let f = crate::closure::js_closure_alloc(code as *const crate::closure::JsFunctionInfo, 0);
    if f.is_null() {
        return;
    }
    class_static_set(class_id, name, crate::value::js_nanbox_pointer(f as i64));
    super::class_registry::class_static_set_defined_attrs(class_id, name, true, false, true);
    // The object's shape now carries "own `name` is this declaration": a
    // process-unique successor, so no object that got `name` any other way
    // (and no other agent's object) shares it (`js_class_static_call_guard`).
    // SAFETY: this agent's live class closure; the bag exists (just stored).
    unsafe {
        super::shapes::transition_object_shape_semantics(crate::closure::props::bag_of(ptr));
    }
}

/// The class function object for `class_id` if this agent has minted it.
/// A read that finds none has its answer without minting one: an object that
/// was never created owns no properties. (A builtin parent such as `Error`
/// never gets a class function object, so reads walking to it must use this.)
#[inline]
pub(crate) fn class_value_if_minted(class_id: u32) -> Option<*mut ClosureHeader> {
    class_value_cached(class_id)
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

/// Captures of a class function object: the class id, the evaluation state
/// ([`CLASS_EVALUATION_STATE_SLOT`]), then the prototype link.
const CLASS_VALUE_CAPTURES: usize = 3;
/// The capture holding the class's `prototype` object (NaN-boxed), or
/// `undefined` before it exists.
const CLASS_PROTOTYPE_LINK_CAPTURE: usize = crate::codegen_abi::CLASS_PROTOTYPE_LINK_CAPTURE;
const _: () = assert!(CLASS_PROTOTYPE_LINK_CAPTURE != CLASS_EVALUATION_STATE_SLOT);
const _: () = assert!(CLASS_EVALUATION_STATE_SLOT < CLASS_VALUE_CAPTURES);
const _: () = assert!(CLASS_PROTOTYPE_LINK_CAPTURE < CLASS_VALUE_CAPTURES);

/// The prototype-link word of class function object `closure`.
///
/// # Safety
/// `closure` is a live class function object minted by [`class_value_mint`].
#[inline]
unsafe fn prototype_link_slot(closure: *mut ClosureHeader) -> *mut u64 {
    crate::closure::closure_capture_slots_mut(closure).add(CLASS_PROTOTYPE_LINK_CAPTURE)
}

/// `C.prototype` of class `class_id` in this agent, or null when it does not
/// exist yet: the link the class's own function object carries.
///
/// `prototype` is an own property of the class constructor
/// (MakeConstructor: `{ [[Writable]]: false, [[Enumerable]]: false,
/// [[Configurable]]: false }`), so the class→prototype link is a fact of the
/// class object, fixed for the agent's life once made. The function object
/// keeps it in a capture next to the class id, so the read is the class-value
/// table's indexed loads plus one: no lock, no hash, no side table. A class
/// whose function object was never minted has no prototype object either
/// (minting builds it, see [`class_value_mint`]).
///
/// `class_id` is the prototype's identity id
/// ([`super::class_registry::decl_prototype_identity_id`]).
#[inline]
pub(crate) fn class_decl_prototype_link(class_id: u32) -> *mut super::ObjectHeader {
    let Some(closure) = class_value_cached(class_id) else {
        return std::ptr::null_mut();
    };
    // SAFETY: a live class function object of this agent.
    let bits = unsafe { *prototype_link_slot(closure) };
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        (bits & crate::value::POINTER_MASK) as *mut super::ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

/// Make `proto` class `class_id`'s prototype link and return the object it
/// replaced (null when none). The class's function object is minted if
/// needed; `proto` must be live and rooted by the caller.
pub(crate) fn class_decl_prototype_link_store(
    class_id: u32,
    proto: *mut super::ObjectHeader,
) -> *mut super::ObjectHeader {
    let previous = class_decl_prototype_link(class_id);
    let closure = class_value_ptr(class_id);
    let bits = crate::value::POINTER_TAG | (proto as u64 & crate::value::POINTER_MASK);
    // GC_STORE_AUDIT(BARRIERED): the class function object is pinned and old; its
    // link slot is a traced capture slot, so the slot barrier remembers a
    // young `proto` for the next minor and shades it for an incremental mark.
    unsafe {
        let slot = prototype_link_slot(closure);
        *slot = bits;
        crate::gc::runtime_write_barrier_slot(closure as usize, slot as usize, bits);
    }
    previous
}

/// Clear every minted class's prototype link (a test resetting the registry).
#[cfg(test)]
pub(crate) fn test_clear_class_decl_prototype_links() {
    for (pages, len) in class_value_dirs() {
        test_clear_band_prototype_links(pages, len);
    }
}

#[cfg(test)]
fn test_clear_band_prototype_links(pages: *mut *mut ClassValuePage, len: usize) {
    for i in 0..len {
        // SAFETY: `pages` holds `len` page pointers (null or a live page).
        let page = unsafe { *pages.add(i) };
        if page.is_null() {
            continue;
        }
        // SAFETY: a live leaked page of this agent.
        for slot in unsafe { (*page).iter() } {
            if !slot.is_null() {
                // SAFETY: a live class function object of this agent.
                unsafe { *prototype_link_slot(*slot) = crate::value::TAG_UNDEFINED };
            }
        }
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

/// `C[prop]` for a class function object at `ptr` (routed by
/// `closure_get_dynamic_prop` on the class ShapeId): the class lookup.
#[cold]
#[inline(never)]
pub(crate) fn class_static_read(ptr: usize, prop: &str, key: *const crate::StringHeader) -> f64 {
    // SAFETY: the caller proved a live class closure (its ShapeId).
    let Some(class_id) = (unsafe { class_closure_id_unchecked(ptr as *const ClosureHeader) })
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    // The class object is pinned: `ptr` survives the key allocation.
    let key = if key.is_null() {
        crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32)
    } else {
        key as *mut crate::StringHeader
    };
    let value = crate::object::field_get_set::class_value_get_field(
        ptr as *const crate::object::ObjectHeader,
        key,
        ptr as u64,
        class_id,
    );
    f64::from_bits(value.bits())
}

/// A statically lowered `C.x` whose compiled alias is detached (`TAG_HOLE`:
/// the static was deleted, redefined as an accessor or made read-only): the
/// generic [[Get]] on the class function object.
///
/// # Safety
/// `name_ptr` points at `name_len` bytes of UTF-8 (codegen rodata).
#[no_mangle]
pub unsafe extern "C" fn js_class_static_field_get(
    class_id: i32,
    name_ptr: *const u8,
    name_len: i64,
) -> f64 {
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    let receiver = class_value_ptr(class_id as u32) as *const crate::object::ObjectHeader;
    f64::from_bits(crate::object::js_object_get_field_by_name(receiver, key).bits())
}

/// A statically lowered `C.x = v` whose compiled alias is detached: the
/// generic [[Set]] on the class function object (a setter, a read-only
/// refusal, or re-creating a deleted static — which re-attaches the alias).
///
/// # Safety
/// As [`js_class_static_field_get`].
#[no_mangle]
pub unsafe extern "C" fn js_class_static_field_put(
    class_id: i32,
    name_ptr: *const u8,
    name_len: i64,
    value: f64,
) {
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    let receiver = class_value_ptr(class_id as u32) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(receiver, key, value);
}

/// The address of class `class_id`'s [[Prototype]] (0 when null): the
/// recorded one (`Object.setPrototypeOf(C, p)`), else the parent class's
/// function object, else the parent function (`extends <function>`), else
/// %Function.prototype%.
pub(crate) fn class_prototype_addr(class_id: u32) -> usize {
    if super::class_registry::class_static_prototype_is_nulled(class_id) {
        return 0;
    }
    let proto = super::class_registry::class_static_prototype(class_id) as usize;
    if proto != 0 {
        proto
    } else if let Some(parent) = super::get_parent_class_id(class_id)
        .filter(|&p| p != 0 && p != class_id && super::is_class_id_registered(p))
    {
        class_value_ptr(parent) as usize
    } else if let Some(parent) = super::class_registry::class_parent_closure(class_id) {
        parent
    } else {
        crate::closure::shape::function_prototype_ptr_materialized()
    }
}

/// [[Get]] of `key` on class `class_id`'s [[Prototype]], `receiver` as the
/// receiver: the continuation of a read of a key the class does not own
/// (e.g. its own `name` was deleted — `Sub.name` then reads `Base.name`, a
/// base class reads `Function.prototype.name`). The [[Prototype]] is the
/// recorded one (`Object.setPrototypeOf(C, p)`), else the parent class's
/// function object, else the parent function (`extends <function>`), else
/// %Function.prototype%.
pub(crate) fn class_prototype_get(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> crate::value::JSValue {
    use crate::value::JSValue;
    // `class_prototype_addr` may build %Function.prototype% (the realm
    // global), which allocates.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_string_ptr(key);
    let proto = class_prototype_addr(class_id);
    if proto == 0 {
        return JSValue::undefined();
    }
    let prev = super::field_get_set::accessor_receiver_override_begin(receiver.get_nanbox_f64());
    let value = key.with_const_ptr::<crate::StringHeader, _>(|key| {
        super::js_object_get_field_by_name(proto as *const super::ObjectHeader, key)
    });
    super::field_get_set::accessor_receiver_override_end(prev);
    value
}

// ---------------------------------------------------------------------------
// Static accessors: accessor properties of the function object.
// ---------------------------------------------------------------------------

/// ClassBody defaults for an accessor: `(enumerable, configurable)`.
pub(crate) const CLASS_ACCESSOR_DEFAULT_ATTRS: (bool, bool) = (false, true);

/// Install — or refresh, when a half arrives later — the ClassBody static
/// accessor `name` of `class_id` as an accessor property of its function
/// object's own-property object: the pair holds the reflected closures and
/// the compiled static entries in the pair's STATIC fields (`fn() -> value`
/// / `fn(v)`, `this` armed by the caller — NOT the instance `fn(this)`
/// convention, so a generic reader of `raw_get`/`raw_set` never sees them).
/// A half whose compiled
/// entry is unchanged keeps its closure, so reflection hands out the same
/// function every time; attributes a `defineProperty` set are kept.
/// Private (`#x`) accessors are not properties and are never installed.
fn install_declared_static_accessor(class_id: u32, name: &str) {
    if name.starts_with('#') {
        return;
    }
    let Some((raw_get, raw_set)) =
        super::class_registry::class_registered_static_accessor_ptrs(class_id, name)
    else {
        return;
    };
    let _no_collect = crate::gc::GcSuppressScope::new();
    let existing = class_static_own_accessor(class_id, name);
    let (have, enumerable, configurable) = match existing {
        Some((acc, e, c)) => (acc, e, c),
        None => (
            crate::object::accessor_pair::Accessor::default(),
            CLASS_ACCESSOR_DEFAULT_ATTRS.0,
            CLASS_ACCESSOR_DEFAULT_ATTRS.1,
        ),
    };
    let half = |raw: usize, have_raw: usize, have: u64, is_setter: bool| -> u64 {
        if raw == 0 {
            0
        } else if raw == have_raw && have != 0 {
            have
        } else {
            let setter_length = if is_setter {
                super::class_registry::class_own_setter_length(class_id, name, true)
            } else {
                None
            };
            super::class_registry::class_accessor_function_value(
                raw,
                is_setter,
                true,
                name,
                setter_length,
            )
            .to_bits()
        }
    };
    let get = half(raw_get, have.static_get, have.get, false);
    let set = half(raw_set, have.static_set, have.set, true);
    class_static_define_accessor(
        class_id,
        name,
        crate::object::accessor_pair::Accessor {
            get,
            set,
            raw_get: 0,
            raw_set: 0,
            static_get: raw_get,
            static_set: raw_set,
        },
        enumerable,
        configurable,
    );
}

/// Class `class_id`'s own accessor property `name` (ClassBody or
/// `defineProperty`), with `(enumerable, configurable)`.
pub(crate) fn class_static_own_accessor(
    class_id: u32,
    name: &str,
) -> Option<(crate::object::accessor_pair::Accessor, bool, bool)> {
    // Static parent walks can reach registered builtin/synthetic ids. Those
    // have their own dispatch and no compiled-class function object to query.
    // Keep class_value_mint's contract intact instead of minting one on a miss.
    if class_id == 0 || class_id >= 0x7FFF_FF00 {
        return None;
    }
    use crate::object::key_attrs as ka;
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: this agent's live class closure; its bag (if any) is a live
    // ordinary object whose attributes live with its keys. Nothing allocates.
    unsafe {
        let bag = crate::closure::props::bag_of(ptr);
        if bag.is_null() {
            return None;
        }
        let entry = ka::object_key_entry(bag, name.as_bytes());
        if entry & ka::ENTRY_ACCESSOR == 0 {
            return None;
        }
        let acc = crate::object::accessor_pair::own_accessor(bag as usize, name.as_bytes())?;
        Some((
            acc,
            entry & ka::ENTRY_NON_ENUMERABLE == 0,
            entry & ka::ENTRY_NON_CONFIGURABLE == 0,
        ))
    }
}

/// Does class `class_id` own an accessor property `name`?
pub(crate) fn class_static_has_own_accessor(class_id: u32, name: &str) -> bool {
    class_static_own_accessor(class_id, name).is_some()
}

/// Define (or replace) class `class_id`'s own accessor property `name`: a data
/// property of that name becomes this accessor.
pub(crate) fn class_static_define_accessor(
    class_id: u32,
    name: &str,
    acc: crate::object::accessor_pair::Accessor,
    enumerable: bool,
    configurable: bool,
) {
    let _no_collect = crate::gc::GcSuppressScope::new();
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: this agent's live class closure; no collection in this scope.
    let bag = unsafe { crate::closure::props::bag_ensure(ptr) };
    crate::object::set_builtin_accessor_pair(
        bag as usize,
        name.to_string(),
        acc,
        crate::object::PropertyAttrs::new(false, enumerable, configurable),
    );
}

/// Change the attributes of class `class_id`'s own accessor `name`.
pub(crate) fn class_static_set_accessor_attrs(
    class_id: u32,
    name: &str,
    enumerable: bool,
    configurable: bool,
) {
    if let Some((acc, _, _)) = class_static_own_accessor(class_id, name) {
        class_static_define_accessor(class_id, name, acc, enumerable, configurable);
    }
}

/// Class `class_id`'s own accessor property names, in creation order.
pub(crate) fn class_static_accessor_names(class_id: u32) -> Vec<String> {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: this agent's live class closure.
    unsafe { crate::closure::props::bag_accessor_names(ptr) }
}

/// Run a class static accessor's getter for `receiver`. The compiled
/// ClassBody entry takes the static convention: `this` is armed (the class
/// the read started from — a stashed override — or `receiver`) and the
/// private/capture owner is `receiver`, the evaluation the getter was found
/// through (#10891/#10893). A `defineProperty` getter is an ordinary closure.
///
/// # Safety
/// `acc` came from [`class_static_own_accessor`].
pub(crate) unsafe fn class_static_accessor_call_get(
    acc: crate::object::accessor_pair::Accessor,
    receiver: f64,
) -> f64 {
    let this = crate::object::field_get_set::accessor_receiver_override_take().unwrap_or(receiver);
    if acc.static_get != 0 {
        crate::object::static_this_arm_if_unarmed(this);
        crate::object::static_private_owner_push(receiver);
        let f = crate::closure::body_call::js_bare_body_fn!(acc.static_get as *const u8;);
        let result = f();
        crate::object::static_private_owner_pop();
        crate::object::static_this_disarm();
        return result;
    }
    if acc.get != 0 {
        return f64::from_bits(crate::object::invoke_accessor_getter(acc.get, this).bits());
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Run a class static accessor's setter; `false` when the accessor has none.
///
/// # Safety
/// As [`class_static_accessor_call_get`].
pub(crate) unsafe fn class_static_accessor_call_set(
    acc: crate::object::accessor_pair::Accessor,
    receiver: f64,
    value: f64,
) -> bool {
    if acc.static_set != 0 {
        crate::object::static_this_arm_if_unarmed(receiver);
        crate::object::static_private_owner_push(receiver);
        let f = crate::closure::body_call::js_bare_body_fn!(acc.static_set as *const u8; value);
        let _ = f(value);
        crate::object::static_private_owner_pop();
        crate::object::static_this_disarm();
        return true;
    }
    if acc.set != 0 {
        crate::object::invoke_accessor_setter(acc.set, receiver, value);
        return true;
    }
    false
}

/// `Object.getOwnPropertyDescriptor(C, name)` for an own accessor of the class.
pub(crate) fn class_static_accessor_descriptor(class_id: u32, name: &str) -> Option<f64> {
    let (acc, enumerable, configurable) = class_static_own_accessor(class_id, name)?;
    let undef = crate::value::TAG_UNDEFINED;
    // SAFETY: both halves are the property's own closure values (or
    // undefined); the builder roots them across its allocation.
    Some(unsafe {
        crate::object::descriptors::build_accessor_descriptor(
            f64::from_bits(if acc.get == 0 { undef } else { acc.get }),
            f64::from_bits(if acc.set == 0 { undef } else { acc.set }),
            enumerable,
            configurable,
        )
    })
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
    // A registered builtin parent has no compiled-class function object.
    // Let the caller continue to its builtin static dispatch on a miss.
    if class_id == 0 || class_id >= 0x7FFF_FF00 {
        return None;
    }
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

/// Define/overwrite class `class_id`'s own static data property `name`: the
/// value only, the key keeps its attributes. Callers performing a [[Set]]
/// have checked `writable` (the attributes live with the key).
pub(crate) fn class_static_set(class_id: u32, name: &str, value: f64) {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above; the bag writers run under a GcSuppressScope.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_set(ptr, name, value);
        } else {
            crate::closure::props::bag_define_value(ptr, name, value);
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

/// `Object.freeze` / `Object.seal` of class `class_id`'s function object:
/// every own string-keyed property becomes non-configurable, and with
/// `drop_writable` every data property non-writable. The attributes are
/// those of the own-property object's keys.
pub(crate) fn class_static_restrict_all(class_id: u32, drop_writable: bool) {
    for (name, _) in class_static_entries(class_id) {
        if let Some((writable, enumerable, _)) =
            super::class_registry::class_static_defined_attrs(class_id, &name)
        {
            super::class_registry::class_static_set_defined_attrs(
                class_id,
                &name,
                writable && !drop_writable,
                enumerable,
                false,
            );
        }
    }
    for name in class_static_accessor_names(class_id) {
        if let Some((_, enumerable, _)) = class_static_own_accessor(class_id, &name) {
            class_static_set_accessor_attrs(class_id, &name, enumerable, false);
        }
    }
}

/// TestIntegrityLevel over class `class_id`'s own string-keyed properties
/// (the object is already known non-extensible): none configurable, and
/// when `frozen` no data property writable.
pub(crate) fn class_static_integrity(class_id: u32, frozen: bool) -> bool {
    for (name, _) in class_static_entries(class_id) {
        let (writable, _, configurable) =
            super::class_registry::class_static_defined_attrs(class_id, &name)
                .unwrap_or((true, true, true));
        if configurable || (frozen && writable) {
            return false;
        }
    }
    class_static_accessor_names(class_id).iter().all(|name| {
        class_static_own_accessor(class_id, name).is_none_or(|(_, _, configurable)| !configurable)
    })
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

    #[test]
    fn static_property_lookups_do_not_mint_builtin_class_values() {
        for cid in [0, 0x7FFF_FF00, 0xFFFF_0024, u32::MAX] {
            assert!(class_static_get(cid, "from").is_none());
            assert!(class_static_own_accessor(cid, "from").is_none());
            assert!(crate::object::class_registry::class_static_prototype(cid).is_null());
            assert!(!crate::object::class_registry::class_static_prototype_is_nulled(cid));
            assert!(class_value_cached(cid).is_none());
        }
    }

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

    /// The class-value table is sized by how many ids of a band exist, never
    /// by an id's magnitude: one class object for the highest id of every
    /// band (fs.Stats' reserved `0xFFFF_0070` among them, which once grew the
    /// directory to 16M pages — 128 MiB zero-filled — on a single
    /// `stats.mtime` read) leaves every directory at most 256 pages.
    #[test]
    fn class_value_directories_are_sized_by_band_offset_not_by_id() {
        let ids = [
            0x7FFF_FF00,
            0x7FFF_FFFF,
            SYNTHETIC_CLASS_ID_BASE,
            0xFFFF_0070, // fs.Stats (`fs::stats::STATS_REGULAR_CLASS_ID`)
            0xFFFF_0071, // bigint fs.Stats
            crate::native_class_ids::CRYPTO_HASH,
            u32::MAX,
        ];
        // Not registered: a registered id is also an INT32 class-ref word
        // (#11414), and `u32::MAX` would turn every `int32(-1)` into a class.
        for cid in ids {
            let v = class_value(cid);
            assert_eq!(class_value_id(v), Some(cid), "{cid:#x} round-trips");
            assert_eq!(class_value(cid).to_bits(), v.to_bits(), "one object per id");
        }
        for (band, (_, len)) in class_value_dirs().into_iter().enumerate() {
            assert!(
                len <= CLASS_VALUE_PAGE_LEN,
                "band {band}: directory of {len} pages (sized by a raw class id)"
            );
        }
        // Distinct ids in distinct bands never share a slot.
        let objs: Vec<u64> = ids.iter().map(|&c| class_value(c).to_bits()).collect();
        let mut unique = objs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), objs.len(), "every band id has its own object");
        // Non-class words have no slot and are never minted.
        assert_eq!(class_value_band(0), None);
        assert_eq!(class_value_band(0x8000_0000), None);
        assert!(class_value_cached(0x8000_0000).is_none());
    }

    /// A static walk that reaches a BUILTIN parent (`class E extends Error`)
    /// stops there: a builtin id has no class function object, so no reader
    /// may mint one for it (minting `0xFFFF_0001` grew the class-value
    /// directory to 16M pages, which every collection then scanned).
    #[test]
    fn a_builtin_parent_never_gets_a_class_function_object() {
        let cid = 0x6D71;
        register(cid);
        crate::object::js_register_class_parent(cid, crate::error::CLASS_ID_ERROR);
        let recv = class_value(cid);
        let pages_before = class_value_dirs().map(|(_, len)| len);
        let applied = unsafe {
            crate::object::class_registry::class_static_accessor_setter_apply(cid, "zz", recv, 1.0)
        };
        assert!(!applied, "no static accessor named zz on the chain");
        assert!(!crate::object::class_registry::static_accessor_in_chain(
            cid, "zz"
        ));
        // A static symbol read up the chain (latch armed, as once any class
        // has a static symbol member) reaches Error too and must not mint.
        crate::symbol::CLASS_STATIC_SYMBOLS_LATCH.arm();
        let sym = unsafe { crate::symbol::js_symbol_new_empty() };
        assert!(crate::symbol::class_static_symbol_lookup_in_chain(cid, sym).is_none());
        assert!(
            crate::symbol::class_static_symbol_keys_for_class(crate::error::CLASS_ID_ERROR)
                .is_empty()
        );
        // Object.getPrototypeOf(C), the static `super` parent value and a
        // static `super[k] = v` all step to the parent id; none may mint.
        let _ = crate::object::js_object_get_prototype_of(recv);
        let _ = crate::object::class_registry::parent_static::template_dynamic_parent_value(cid);
        let _ = crate::proxy::js_super_put_value_set(
            crate::error::CLASS_ID_ERROR,
            crate::value::js_nanbox_string(
                crate::string::js_string_from_bytes(b"zz".as_ptr(), 2) as i64
            ),
            1.0,
            recv,
            0,
        );
        assert!(
            class_value_cached(crate::error::CLASS_ID_ERROR).is_none(),
            "the builtin Error id must not get a class function object"
        );
        let pages_after = class_value_dirs().map(|(_, len)| len);
        assert_eq!(
            pages_after, pages_before,
            "the walks grew the class-value directory"
        );
    }

    /// A class function object is traced like any other pinned object: a full
    /// collection reaches its own-property bag (the statics) through the
    /// pinned-object root scan, with no class-value scanner.
    #[test]
    fn a_full_collection_keeps_the_class_statics_bag() {
        let cid = 0x6B02;
        register(cid);
        // A unit-test thread may not have run `gc_init`'s scanner list.
        crate::gc::gc_register_mutable_root_scanner(crate::gc::scan_pinned_object_roots_mut);
        let text = "static-payload-11609";
        let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        class_static_set(
            cid,
            "k11609",
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits()),
        );
        crate::gc::js_gc_collect();
        crate::gc::js_gc_collect();
        let got = class_static_get(cid, "k11609").expect("the static survives a full collection");
        let got = crate::value::JSValue::from_bits(got.to_bits());
        let hdr = got.as_string_ptr();
        assert!(!hdr.is_null());
        let bytes = unsafe { crate::string::OwnedStringBytes::copy_from_header(hdr) };
        assert_eq!(bytes.as_bytes(), text.as_bytes());
        let keys: Vec<String> = class_static_entries(cid)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(keys.iter().any(|k| k == "k11609"), "own keys: {keys:?}");
    }

    /// The prototype link (capture 2) and the statics bag of an OLD, pinned
    /// class hold young objects written after its birth: the slot barrier
    /// remembers them, so a moving minor keeps them alive and rewrites both
    /// edges. Dropping the barrier in `class_decl_prototype_link_store` makes
    /// the link read back a stale address.
    #[test]
    fn young_values_in_an_old_class_survive_moving_minors() {
        let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        let cid = 0x6B04;
        register(cid);
        let _ = class_value_ptr(cid);
        // The class is old and pinned from birth; everything below is young.
        let proto = crate::object::js_object_alloc(0x6B05, 1);
        crate::object::js_object_set_field(
            proto,
            0,
            crate::value::JSValue::from_bits(crate::value::INT32_TAG | 4242),
        );
        let before = proto as usize;
        class_decl_prototype_link_store(cid, proto);
        let text = "young-static-6b04";
        let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        class_static_set(
            cid,
            "k6b04",
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits()),
        );
        for _ in 0..3 {
            let _ = crate::gc::gc_collect_minor();
        }
        let linked = class_decl_prototype_link(cid);
        assert!(!linked.is_null());
        assert_ne!(
            linked as usize, before,
            "forced evacuation must have moved the young prototype"
        );
        let field = crate::object::js_object_get_field(linked, 0);
        assert_eq!(field.bits(), crate::value::INT32_TAG | 4242);
        let got = class_static_get(cid, "k6b04").expect("the static survives minors");
        let hdr = crate::value::JSValue::from_bits(got.to_bits()).as_string_ptr();
        assert!(!hdr.is_null());
        let bytes = unsafe { crate::string::OwnedStringBytes::copy_from_header(hdr) };
        assert_eq!(bytes.as_bytes(), text.as_bytes());
    }

    /// Write once, then survive several minors with NO further writes. With
    /// the promotion age pinned at 4 the young prototype stays young across
    /// minors 1..=3, so the class (old, pinned) must stay in the minor scan
    /// set the whole time: an entry is re-armed by every scan that still finds
    /// a young referent, not consumed after one minor. Returns false (without
    /// reading the possibly stale link) the first time the entry is missing.
    fn write_once_then_minors(cid: u32, sabotage_drop_entry: bool) -> bool {
        let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        let _age = crate::gc::pin_tenuring_survivals_for_test(4);
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        register(cid);
        let closure = class_value_ptr(cid);
        let proto = crate::object::js_object_alloc(cid + 1, 1);
        crate::object::js_object_set_field(
            proto,
            0,
            crate::value::JSValue::from_bits(crate::value::INT32_TAG | 777),
        );
        class_decl_prototype_link_store(cid, proto);
        let slot = unsafe { prototype_link_slot(closure) } as usize;
        assert!(
            crate::gc::old_slot_page_is_remembered_for_test(slot),
            "the write must have remembered the class"
        );
        let _sabotage = sabotage_drop_entry.then(|| {
            crate::gc::copy_decode_sabotage::Guard::arm(crate::gc::copy_decode_sabotage::CHILD)
        });
        let mut moved_from = proto as usize;
        for minor in 1..=3 {
            let _ = crate::gc::gc_collect_minor();
            // No write since the first store. The referent must still be young
            // (age < 4), so the entry must still be present.
            if !crate::gc::old_slot_page_is_remembered_for_test(slot) {
                return false;
            }
            let linked = class_decl_prototype_link(cid);
            assert!(!linked.is_null());
            assert!(
                matches!(
                    crate::arena::classify_heap_generation(linked as usize),
                    crate::arena::HeapGeneration::Nursery
                ),
                "minor {minor}: the referent must still be young"
            );
            assert_ne!(
                linked as usize, moved_from,
                "minor {minor}: it must have moved"
            );
            moved_from = linked as usize;
            let field = crate::object::js_object_get_field(linked, 0);
            assert_eq!(field.bits(), crate::value::INT32_TAG | 777, "minor {minor}");
        }
        // The fourth survival promotes it; the value is intact afterwards too.
        let _ = crate::gc::gc_collect_minor();
        let linked = class_decl_prototype_link(cid);
        let field = crate::object::js_object_get_field(linked, 0);
        assert_eq!(
            field.bits(),
            crate::value::INT32_TAG | 777,
            "after promotion"
        );
        true
    }

    #[test]
    fn a_class_stays_remembered_while_its_referent_is_young() {
        assert!(write_once_then_minors(0x6B10, false));
    }

    /// Sabotage: the scan loses the child, so the entry is dropped after the
    /// first minor. The run must go red: either the collector's own
    /// post-cycle coverage check fires (the expected panic) or the witness
    /// above sees the entry missing and returns false (the `assert!` panic).
    #[test]
    #[should_panic]
    fn dropping_the_entry_after_one_minor_is_caught() {
        assert!(write_once_then_minors(0x6B20, true));
    }

    /// The kind is a shape fact: class function objects carry their own
    /// ShapeId, distinct from FunctionDictionary (shapes are canonical per
    /// facts — without its marker fact the class shape WOULD be the dictionary
    /// id and every dictionary function would route as a class), and it is
    /// sticky across own-property installs.
    #[test]
    fn class_function_objects_have_their_own_sticky_shape() {
        let cid = 0x6C01;
        register(cid);
        let class_shape = crate::closure::shape::function_class_shape();
        assert_ne!(
            class_shape,
            crate::closure::shape::function_dictionary_shape()
        );
        let ptr = class_value_ptr(cid);
        assert_eq!(unsafe { (*ptr).shape_id }, class_shape);
        class_static_set(cid, "s", 1.0);
        crate::closure::shape::note_function_own_state_changed(ptr as usize);
        assert_eq!(unsafe { (*ptr).shape_id }, class_shape, "sticky");
        extern "C" fn body(_: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
            0.0
        }
        let f = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
        crate::closure::shape::note_function_own_state_changed(f as usize);
        assert_ne!(
            unsafe { (*f).shape_id },
            class_shape,
            "a dictionary function is not a class"
        );
    }

    /// A class constructor's `length` and `name` are own data properties of
    /// its function object (in its own-property object, intrinsic
    /// attributes); a static method of that name owns the key instead.
    #[test]
    fn name_and_length_are_own_data_of_the_function_object() {
        let cid = 0x6D01;
        register(cid);
        unsafe { crate::object::js_register_class_name(cid, b"Zed".as_ptr(), 3) };
        crate::object::js_register_class_length(cid, 2);
        let ptr = class_value_ptr(cid) as usize;
        assert_eq!(
            unsafe { crate::closure::props::bag_get(ptr, b"length") },
            Some(2.0),
            "own length"
        );
        let name = unsafe { crate::closure::props::bag_get(ptr, b"name") }.expect("own name");
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        // SAFETY: a live string value just read from the object.
        let bytes = unsafe {
            crate::string::js_string_key_bytes(
                crate::value::JSValue::from_bits(name.to_bits()),
                &mut scratch,
            )
        }
        .expect("a string");
        assert_eq!(bytes, b"Zed");
        for key in ["length", "name"] {
            assert_eq!(
                crate::object::class_registry::class_static_defined_attrs(cid, key),
                Some(INTRINSIC_ATTRS),
                "{key}: non-writable, non-enumerable, configurable"
            );
        }
        extern "C" fn static_name() -> f64 {
            0.0
        }
        unsafe {
            crate::object::class_registry::js_register_class_static_method(
                cid as i64,
                b"name".as_ptr(),
                4,
                static_name as *const () as usize as i64,
                0,
                0,
            )
        };
        extern "C" fn static_name_entry(_closure: i64) -> f64 {
            0.0
        }
        unsafe {
            crate::object::class_registry::parent_static::js_register_class_static_method_entry(
                cid as i64,
                b"name".as_ptr(),
                4,
                static_name_entry as *const () as usize as i64,
            )
        };
        // node: `class Zed { static name() {} }` -> Zed.name is the method, an
        // own data property { writable, !enumerable, configurable }.
        let method = unsafe { crate::closure::props::bag_get(ptr, b"name") }
            .expect("a static method named `name` is the class's own `name`");
        assert_eq!(
            unsafe { static_method_code_of(method) },
            Some(static_name_entry as *const () as usize),
            "its value is the method's function object"
        );
        assert_eq!(
            crate::object::class_registry::class_static_defined_attrs(cid, "name"),
            Some((true, false, true)),
            "method attributes, not the intrinsic's"
        );
    }

    /// A ClassBody static accessor is an accessor property of the class
    /// function object: ClassBody attributes, one closure per half across
    /// reads, attributes changed in place, and a delete removes it.
    #[test]
    fn static_accessors_are_accessor_properties_of_the_function_object() {
        let cid = 0x6E01;
        register(cid);
        extern "C" fn getter() -> f64 {
            41.0
        }
        unsafe {
            crate::object::js_register_class_name(cid, b"Acc".as_ptr(), 3);
            crate::object::class_registry::js_register_class_static_getter(
                cid as i64,
                b"g".as_ptr(),
                1,
                getter as *const () as usize as i64,
            );
        }
        let (acc, enumerable, configurable) =
            class_static_own_accessor(cid, "g").expect("an own accessor property");
        assert_ne!(acc.get, 0, "a reflected getter closure");
        assert_eq!(acc.set, 0);
        assert_eq!((enumerable, configurable), CLASS_ACCESSOR_DEFAULT_ATTRS);
        let again = class_static_own_accessor(cid, "g").unwrap().0;
        assert_eq!(again.get, acc.get, "one closure per half");
        let ptr = class_value_ptr(cid) as usize;
        assert_eq!(
            unsafe { crate::closure::props::bag_get(ptr, b"g") },
            None,
            "an accessor key has no data value"
        );
        let got = unsafe { class_static_accessor_call_get(acc, class_value(cid)) };
        assert_eq!(got, 41.0);
        class_static_set_accessor_attrs(cid, "g", true, false);
        assert_eq!(
            class_static_own_accessor(cid, "g").map(|(_, e, c)| (e, c)),
            Some((true, false))
        );
        class_static_set_accessor_attrs(cid, "g", false, true);
        assert!(class_static_remove(cid, "g"), "delete removes the property");
        assert!(class_static_own_accessor(cid, "g").is_none());
    }

    /// Only the function object's own code pointer names a class.
    #[test]
    fn ordinary_closures_and_numbers_are_not_class_values() {
        extern "C" fn body(_: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
            0.0
        }
        let c = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
        assert_eq!(class_closure_id(c as usize), None);
        assert_eq!(class_value_id(42.0), None);
        assert_eq!(
            class_value_id(f64::from_bits(crate::value::TAG_UNDEFINED)),
            None
        );
    }
}

/// Expose the existing class function-object directory, without another registry.
#[no_mangle]
pub extern "C" fn perry_class_value_dir_cell() -> *const u8 {
    CLASS_VALUES.with(|bands| {
        let p = bands[0].as_ptr() as *const u8;
        crate::agent_ptrs::publish(crate::codegen_abi::AGENT_PTR_CLASS_VALUES, p);
        p
    })
}

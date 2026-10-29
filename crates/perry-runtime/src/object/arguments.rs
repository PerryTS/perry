//! ECMAScript Arguments objects with ordinary property slots.
//!
//! A private allocation tail carries the birth layout and mapped aliases.
//! It moves and dies with the object; birth needs no metadata allocation.

use super::descriptors::{build_accessor_descriptor, build_data_descriptor};
use super::header::{meta_form_alloc_bytes, MetaObj};
use super::*;
use crate::codegen_abi::LayoutWord;

#[repr(C)]
struct ArgumentsTail {
    len: u32,
    /// Immutable birth layout; the live shape must still equal this identity.
    birth_shape: LayoutWord,
    /// TAG_TRUE / TAG_FALSE for unmapped objects; a boxed alias array otherwise.
    state: u64,
}

/// Only called on an allocation born by unstamped_arguments_object.
unsafe fn arguments_tail(obj: *const ObjectHeader) -> *mut ArgumentsTail {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    (header as *mut u8)
        .add((*header).size as usize - std::mem::size_of::<ArgumentsTail>())
        .cast()
}

/// Explicit child edge outside the ordinary property-slot range. The GC
/// enumerates it even when a property mutation changes the receiver's shape.
pub(crate) unsafe fn arguments_mapping_slot(obj: *mut ObjectHeader) -> Option<*mut u64> {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    ((*header)._reserved & crate::gc::OBJ_FLAG_ARGUMENTS != 0)
        .then(|| &mut (*arguments_tail(obj)).state as *mut u64)
}

const ARGUMENTS_RESTRICTED: u64 = crate::value::TAG_TRUE;
const ARGUMENTS_UNMAPPED: u64 = crate::value::TAG_FALSE;

crate::perry_thread_local! {
    // Bounded, agent-local cache of immutable ordered keys, one per arity and
    // `callee` kind (index `len` for a mapped/sloppy `callee`, `65 + len` for a
    // restricted one). The attributes of `length` and `callee` are part of the
    // list (charter step 3, `key_attrs.rs`), so an arguments object is born
    // with its final layout. Values, accessor closures and mapped boxes still
    // belong to each individual arguments object.
    static ARGUMENTS_KEYS: RefCell<[*mut ArrayHeader; 130]> =
        RefCell::new([std::ptr::null_mut(); 130]);
    // The accessor pair every restricted `callee` holds: the %ThrowTypeError%
    // singleton as both getter and setter. A pair is immutable once built
    // (`accessor_pair::pair_new` is its only writer) and `callee` is
    // non-configurable, so one pair serves every strict arguments object.
    static RESTRICTED_CALLEE_PAIR: RefCell<*mut ArrayHeader> =
        RefCell::new(std::ptr::null_mut());
    // The ShapeId an arguments object of each `ARGUMENTS_KEYS` slot was last
    // born with (0 = none yet). A ShapeId is a number, never reused, and
    // `shapes::try_birth_stamp_preinstalled_shape` re-checks every structural
    // fact against the live descriptor, so a stale or pruned id only costs
    // one publish. The word is dereferenced by that re-check, so the cache is
    // a cache carrier (note_arguments_shape_carriers): a held word is never
    // retired while this table names it.
    static ARGUMENTS_SHAPES: RefCell<[u32; 130]> = RefCell::new([0; 130]);
}

/// Latched by arguments birth. Twin of `set::SET_REGISTRY_EVER_USED` /
/// `map::MAP_REGISTRY_EVER_USED`, and added for the same reason:
/// [`is_arguments_object`] is a *probe*, run on paths that have nothing to do
/// with `arguments` — the by-name property-get tail, `Array.prototype.push`,
/// the array and `Symbol.iterator` iterator entries, `Array.from`/`concat`,
/// class construction — so a program that never writes the identifier
/// `arguments` still paid for the lookup on each of them. With the old
/// address-keyed registry that was a thread-local resolution plus a `RefCell`
/// borrow plus a pointer hash, and on the `interp` benchmark **2.8% of the
/// whole program** proving the absence of a feature the source does not
/// contain. The meta-record lookup that replaced it (#11506) is cheaper, but it
/// still reads the receiver's GC header and metadata record, which the latch
/// keeps off every such path.
///
/// Process-global rather than per-thread on purpose: Darwin has no local-exec
/// TLS, so reading a `thread_local!` flag would cost the very `_tlv_get_addr`
/// call this exists to avoid. Being global only makes it *conservative* — one
/// thread creating an arguments object sends every thread back to the real
/// lookup, which is the pre-latch behaviour.
static ARGUMENTS_OBJECTS_EVER_USED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// True when no arguments object has ever been created, so
/// [`is_arguments_object`] can answer without touching the receiver.
#[inline(always)]
fn arguments_registry_never_used() -> bool {
    !ARGUMENTS_OBJECTS_EVER_USED.load(std::sync::atomic::Ordering::Relaxed)
}

/// What an object's private arguments tail says about it.
#[derive(Clone, Copy)]
enum ArgumentsState {
    Restricted,
    Unmapped,
    Mapped(*mut ArrayHeader),
}

impl ArgumentsState {
    fn restricted_callee(self) -> bool {
        matches!(self, ArgumentsState::Restricted)
    }

    /// The cell parameter `index` still aliases, if any. Never allocates. The
    /// cell is movable, so the answer is valid only until the next collection
    /// point; callers re-derive it after anything that can allocate.
    unsafe fn mapped_box(self, index: u32) -> Option<*mut crate::r#box::Box> {
        let ArgumentsState::Mapped(map) = self else {
            return None;
        };
        if index >= (*map).length {
            return None;
        }
        let bits = *(crate::array::array_elements_ptr(map) as *const u64).add(index as usize);
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        let cell = (bits & crate::value::POINTER_MASK) as usize as *mut crate::r#box::Box;
        (!cell.is_null()).then_some(cell)
    }

    /// Break index `index`'s alias. Never allocates.
    unsafe fn unmap(self, index: u32) {
        let ArgumentsState::Mapped(map) = self else {
            return;
        };
        if index < (*map).length {
            // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is a non-pointer sentinel.
            *(crate::array::array_elements_ptr(map) as *mut u64).add(index as usize) =
                crate::value::TAG_HOLE;
        }
    }
}

/// `obj`'s arguments state, read from its private tail. Never allocates.
/// `obj` may be any pointer a probe was handed: a non-object, a handle, or an
/// ordinary object without the arguments identity all answer `None`.
///
/// # Safety
/// `obj` is a live heap address or arbitrary non-pointer bits (the
/// `try_read_gc_header` contract).
#[inline]
unsafe fn arguments_state(obj: *const ObjectHeader) -> Option<ArgumentsState> {
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    if header._reserved & crate::gc::OBJ_FLAG_ARGUMENTS == 0 {
        return None;
    }
    match (*arguments_tail(obj)).state {
        0 => None,
        ARGUMENTS_RESTRICTED => Some(ArgumentsState::Restricted),
        ARGUMENTS_UNMAPPED => Some(ArgumentsState::Unmapped),
        bits => Some(ArgumentsState::Mapped(
            JSValue::from_bits(bits).as_pointer::<ArrayHeader>() as *mut ArrayHeader,
        )),
    }
}

/// The shared per-arity key lists are the only runtime-held state left here;
/// everything per object lives in its allocation.
pub fn scan_arguments_object_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ARGUMENTS_KEYS.with(|cache| {
        for keys in cache.borrow_mut().iter_mut() {
            visitor.visit_raw_mut_ptr_slot(keys);
        }
    });
    RESTRICTED_CALLEE_PAIR.with(|pair| visitor.visit_raw_mut_ptr_slot(&mut *pair.borrow_mut()));
}

/// Claim every layout word the birth cache still names (full-trace recompute).
pub(crate) fn note_arguments_shape_carriers() {
    ARGUMENTS_SHAPES.with(|ids| {
        for &id in ids.borrow().iter().filter(|&&id| id != 0) {
            crate::object::shape_carriers::note_shape_id(LayoutWord(id));
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_arguments_object_roots() {
    ARGUMENTS_KEYS.with(|cache| cache.borrow_mut().fill(std::ptr::null_mut()));
    ARGUMENTS_SHAPES.with(|ids| *ids.borrow_mut() = [0; 130]);
    RESTRICTED_CALLEE_PAIR.with(|pair| *pair.borrow_mut() = std::ptr::null_mut());
}

/// The shared restricted-`callee` accessor pair, built on first use.
fn restricted_callee_pair() -> *mut ArrayHeader {
    let cached = RESTRICTED_CALLEE_PAIR.with(|pair| *pair.borrow());
    if !cached.is_null() {
        return cached;
    }
    let thrower = thrower_closure_value();
    let pair = unsafe {
        super::accessor_pair::pair_new(super::accessor_pair::pair_from(&AccessorDescriptor {
            get: thrower.to_bits(),
            set: thrower.to_bits(),
        }))
    };
    RESTRICTED_CALLEE_PAIR.with(|slot| {
        // GC_STORE_AUDIT(ROOT): the arguments scanner traces and rewrites this slot.
        unsafe { crate::gc::runtime_store_root_raw_mut_ptr_slot(&mut *slot.borrow_mut(), pair) };
    });
    pair
}

/// Test-only: the box parameter `index` of `obj` aliases.
#[cfg(test)]
pub(crate) fn test_arguments_mapped_box(obj: *const ObjectHeader, index: u32) -> Option<usize> {
    unsafe { arguments_state(obj)?.mapped_box(index) }.map(|b| b as usize)
}

/// Test-only: the mapping array a sloppy mapped object's tail points at.
#[cfg(test)]
pub(crate) fn test_arguments_mapping_array(obj: *const ObjectHeader) -> Option<*mut ArrayHeader> {
    match unsafe { arguments_state(obj) }? {
        ArgumentsState::Mapped(map) => Some(map),
        ArgumentsState::Restricted | ArgumentsState::Unmapped => None,
    }
}

fn key_name(key: *const crate::StringHeader) -> Option<String> {
    if key.is_null() || (key as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

fn intern_key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

extern "C" fn arguments_throw_type_error(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    super::throw_object_type_error(
        b"'caller', 'callee', and 'arguments' properties may not be accessed",
    );
}

pub(super) const THROWER_FROZEN_FLAGS: u16 =
    crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;

pub(super) fn thrower_closure_value() -> f64 {
    let closure = crate::closure::js_closure_alloc_singleton(
        crate::fn_info!(#[realm("Function.prototype.arguments.<get>", integrity = frozen, spec = {name = ""}, emit = metadata(realm_thrower_public))] arguments_throw_type_error, 0; with_declared(0)),
    );
    // #10509: configure the per-thread singleton once, not per strict call.
    // The frozen bits are the last step below and live in the closure's own
    // header; every side table the earlier steps write (closure props,
    // descriptor attrs, builtin-closure metadata) is rekeyed when the closure
    // moves, so a set bit means the whole configuration is still in place.
    if !closure.is_null()
        && unsafe { crate::value::addr_class::try_read_tracked_gc_header(closure as usize) }
            .is_some_and(|gc| unsafe {
                (*gc.as_ptr())._reserved & THROWER_FROZEN_FLAGS == THROWER_FROZEN_FLAGS
            })
    {
        return crate::value::js_nanbox_pointer(closure as i64);
    }
    super::native_module::set_bound_native_closure_name(closure, realm_thrower_public!(name));
    super::native_module::set_builtin_closure_length(closure as usize, 0);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let frozen = PropertyAttrs::new(false, false, false);
    set_property_attrs(closure as usize, "name".to_string(), frozen);
    set_property_attrs(closure as usize, "length".to_string(), frozen);
    if !closure.is_null() {
        unsafe {
            if let Some(gc) = crate::value::addr_class::try_read_tracked_gc_header(closure as usize)
            {
                (*gc.as_ptr())._reserved |= THROWER_FROZEN_FLAGS;
            }
        }
    }
    crate::value::js_nanbox_pointer(closure as i64)
}

/// The attribute entry of a mapped/sloppy arguments object's `length` and
/// `callee`: `{ writable: true, enumerable: false, configurable: true }`.
const HIDDEN_DATA_ENTRY: u8 = super::key_attrs::ENTRY_NON_ENUMERABLE;

/// The entry of a restricted `callee`: a getter/setter pair (the thrower),
/// `{ enumerable: false, configurable: false }`.
const RESTRICTED_CALLEE_ENTRY: u8 = super::key_attrs::ENTRY_ACCESSOR
    | super::key_attrs::ENTRY_HAS_GET
    | super::key_attrs::ENTRY_HAS_SET
    | super::key_attrs::ENTRY_NON_WRITABLE
    | super::key_attrs::ENTRY_NON_ENUMERABLE
    | super::key_attrs::ENTRY_NON_CONFIGURABLE;

/// Build the complete own-key layout — the indices, then `length` and
/// `callee` WITH their attributes — once per arity and callee kind for common
/// arities. It is a canonical list (`canonical_keys.rs`), so every arguments
/// object of one arity shares it and nothing is copied per call. Larger calls
/// find the same canonical list without retaining a cache slot.
fn arguments_keys(len: u32, restricted_callee: bool) -> *mut ArrayHeader {
    let slot_index = len as usize + if restricted_callee { 65 } else { 0 };
    let cacheable = len <= 64;
    if cacheable {
        if let Some(keys) = ARGUMENTS_KEYS.with(|cache| cache.borrow().get(slot_index).copied()) {
            if !keys.is_null() {
                return keys;
            }
        }
    }
    let proof = super::canonical_keys::SharedLayout::shape_cache_entry();
    let scope = crate::gc::RuntimeHandleScope::new();
    let list = scope.root_raw_mut_ptr::<ArrayHeader>(std::ptr::null_mut());
    let mut cur = super::canonical_keys::CanonicalKeys::EMPTY;
    let mut append = |name: &str, entry: u8| {
        let key = intern_key(name);
        // SAFETY: the parent is rooted in `list`; the key is live and
        // `extend_key_with_entry` roots both across its allocation.
        let next = list.with_mut_ptr(|arr: *mut ArrayHeader| unsafe {
            super::canonical_keys::extend_key_with_entry(&proof, cur.rooted_at(arr), key, entry)
        });
        list.set_raw_mut_ptr(next.as_ptr());
        cur = next;
    };
    for i in 0..len {
        append(&i.to_string(), 0);
    }
    append("length", HIDDEN_DATA_ENTRY);
    append(
        "callee",
        if restricted_callee {
            RESTRICTED_CALLEE_ENTRY
        } else {
            HIDDEN_DATA_ENTRY
        },
    );
    list.with_mut_ptr(|keys: *mut ArrayHeader| {
        if cacheable {
            ARGUMENTS_KEYS.with(|cache| {
                if let Some(slot) = cache.borrow_mut().get_mut(slot_index) {
                    // GC_STORE_AUDIT(ROOT): the arguments scanner traces and rewrites this slot.
                    unsafe { crate::gc::runtime_store_root_raw_mut_ptr_slot(slot, keys) };
                }
            });
        }
        keys
    })
}

#[no_mangle]
pub extern "C" fn js_arguments_object_alloc(
    raw_args: f64,
    callee: f64,
    restricted_callee: i32,
) -> *mut ObjectHeader {
    arguments_object_alloc(raw_args, callee, restricted_callee != 0, 0)
}

/// A sloppy MAPPED arguments object (#11506): [`js_arguments_object_alloc`]
/// with an unrestricted `callee`, plus room for `mapped_count` parameter
/// aliases, which the prologue then fills with
/// [`js_arguments_object_map_index`]. CreateMappedArgumentsObject maps only
/// the indices the call actually passed (`index < len`), so the room is
/// `min(mapped_count, len)`.
///
/// The room is allocated HERE and never in `map_index`: the prologue holds
/// the object in a bare register across its `map_index` calls, so those
/// calls must not be able to collect.
#[no_mangle]
pub extern "C" fn js_arguments_object_alloc_mapped(
    raw_args: f64,
    callee: f64,
    mapped_count: u32,
) -> *mut ObjectHeader {
    arguments_object_alloc(raw_args, callee, false, mapped_count)
}

/// A fresh arguments object with `slots` inline slots, every slot
/// `undefined`, no metadata record and NO shape yet: the same raw birth
/// `js_object_alloc_with_shape` performs, so the caller can stamp the shape
/// the object is born with instead of publishing an empty one first.
fn unstamped_arguments_object(slots: u32) -> *mut ObjectHeader {
    let alloc_slots = std::cmp::max(slots as usize, crate::object::INLINE_SLOT_FLOOR);
    let obj = crate::arena::arena_alloc_gc(
        meta_form_alloc_bytes(alloc_slots) + std::mem::size_of::<ArgumentsTail>(),
        8,
        crate::gc::GC_TYPE_OBJECT,
    ) as *mut ObjectHeader;
    unsafe {
        // Arm the existing probe latch before publishing the arguments identity.
        ARGUMENTS_OBJECTS_EVER_USED.store(true, std::sync::atomic::Ordering::Relaxed);
        (*obj).class_id = 0;
        let header = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*header)._reserved |= crate::gc::OBJ_FLAG_ARGUMENTS;
        // GC_STORE_AUDIT(INIT): private tail starts pointer-free, before any mint.
        arguments_tail(obj).write(ArgumentsTail {
            len: slots - 2,
            birth_shape: LayoutWord(0),
            state: ARGUMENTS_UNMAPPED,
        });
        (*obj).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record.
        *crate::object::header::MetaObj::runtime_born(obj).meta_slot() = std::ptr::null_mut();
        let fields = MetaObj::runtime_born(obj).slots() as *mut JSValue;
        for i in 0..alloc_slots {
            // GC_STORE_AUDIT(INIT): freshly allocated slot, initialized pointer-free.
            std::ptr::write(fields.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(obj as *mut u8);
    }
    obj
}

fn arguments_object_alloc(
    raw_args: f64,
    callee: f64,
    restricted_callee: bool,
    mapped_count: u32,
) -> *mut ObjectHeader {
    let addr = crate::value::js_nanbox_get_pointer(raw_args) as *const ObjectHeader;
    if is_arguments_object(addr) {
        // The immutable body info selected exactly this birth ABI. Its object
        // is already rooted in the synthetic parameter slot at body entry.
        return addr as *mut ObjectHeader;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_args = scope.root_nanbox_f64(raw_args);
    let callee = scope.root_nanbox_f64(callee);
    let arr_ptr = crate::array::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
        raw_args.get_nanbox_f64(),
    ) as *const ArrayHeader);
    let len = if arr_ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr_ptr)
    };

    // Snapshot before birth: even a descriptor-bearing input Array can run
    // getters. Every earlier value remains rooted across the next read.
    let values: Vec<_> = (0..len)
        .map(|i| {
            let arr = crate::array::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
                raw_args.get_nanbox_f64(),
            ) as *const ArrayHeader);
            scope.root_nanbox_f64(f64::from_bits(crate::array::js_array_get(arr, i).bits()))
        })
        .collect();
    arguments_object_alloc_rooted(
        &values,
        callee.get_nanbox_f64(),
        restricted_callee,
        mapped_count,
    )
}

/// Birth straight from the caller's rooted values; no rest Array.
pub(crate) fn arguments_object_alloc_rooted(
    values: &[crate::gc::RuntimeHandle<'_>],
    callee: f64,
    restricted_callee: bool,
    mapped_count: u32,
) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(callee);
    let len = u32::try_from(values.len()).expect("arguments length fits u32");
    let keys = scope.root_raw_mut_ptr(arguments_keys(len, restricted_callee));
    // The restricted `callee` pair, built (once per thread) before the
    // newborn is stamped: nothing may allocate between a birth stamp and the
    // initialization of the fields.
    let pair = restricted_callee.then(|| scope.root_raw_mut_ptr(restricted_callee_pair()));
    let slots = len.saturating_add(2);
    let obj = scope.root_raw_mut_ptr(unstamped_arguments_object(slots));
    let cache_index = if len <= 64 {
        Some(len as usize + if restricted_callee { 65 } else { 0 })
    } else {
        None
    };
    obj.with_mut_ptr(|obj| {
        keys.with_mut_ptr(|keys| unsafe {
            // The cached list is canonical: its own count is `len + 2`, and a
            // longer list may share its backing, so the count is stated.
            let keys = crate::object::ObjectKeys::new(keys, slots);
            let cached = ARGUMENTS_SHAPES
                .with(|ids| cache_index.and_then(|i| ids.borrow().get(i).copied()))
                .unwrap_or(0);
            // One arity and callee kind is one shape: stamp it straight from
            // its descriptor, which re-checks keys, count, width, kind and
            // prototype identity. A miss (first birth, a pruned id) publishes
            // and remembers the id.
            if cached == 0
                || !crate::object::shapes::try_birth_stamp_preinstalled_shape(
                    obj,
                    LayoutWord(cached),
                    keys,
                    slots,
                )
            {
                set_object_keys_with_live(obj, keys, slots);
                let id = crate::object::shapes::object_shape_stamp(obj);
                ARGUMENTS_SHAPES.with(|ids| {
                    if let Some(index) = cache_index {
                        ids.borrow_mut()[index] = id.0;
                        crate::object::shape_carriers::note_shape_id(id);
                    }
                });
            }
        });
    });
    // A newborn has no observer and no collection can occur during these
    // stores. Initialize physical slots once, then publish their GC layout.
    obj.with_mut_ptr(|obj| unsafe {
        let fields = MetaObj::runtime_born(obj).slots();
        for (i, value) in values.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): rooted values; stores cannot collect.
            fields.add(i).write(value.get_nanbox_f64().to_bits());
        }
        // GC_STORE_AUDIT(INIT): newborn property slots, finalized below.
        fields.add(len as usize).write((len as f64).to_bits());
        fields
            .add(len as usize + 1)
            .write(pair.as_ref().map_or_else(
                || callee.get_nanbox_f64().to_bits(),
                |pair| {
                    crate::value::js_nanbox_pointer(pair.get_raw_mut_ptr::<ArrayHeader>() as i64)
                        .to_bits()
                },
            ));
        (*arguments_tail(obj)).birth_shape =
            LayoutWord(crate::object::shapes::object_shape_stamp(obj).0);
        // Attributes are already in the birth shape. No empty descriptor edit.
        let header = (obj as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
        crate::object::rebuild_object_field_layout(obj, slots as usize);
    });

    // The mapping room: one `TAG_HOLE` per mappable index until the prologue
    // fills it. An unmapped object (restricted `callee`) never has one.
    let map_len = if restricted_callee {
        0
    } else {
        mapped_count.min(len)
    };
    let map = scope.root_raw_mut_ptr::<ArrayHeader>(std::ptr::null_mut());
    if map_len > 0 {
        map.set_raw_mut_ptr(crate::array::js_array_alloc_with_length_exact(map_len));
    }

    let obj = obj.get_raw_mut_ptr::<ObjectHeader>();
    let state = if restricted_callee {
        ARGUMENTS_RESTRICTED
    } else if map.get_raw_mut_ptr::<ArrayHeader>().is_null() {
        ARGUMENTS_UNMAPPED
    } else {
        crate::value::js_nanbox_pointer(map.get_raw_mut_ptr::<ArrayHeader>() as i64).to_bits()
    };
    unsafe {
        let slot = &mut (*arguments_tail(obj)).state as *mut u64;
        // GC_STORE_AUDIT(BARRIERED): explicit mapped-alias child edge.
        slot.write(state);
        crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, state);
    }
    obj
}

/// #10509: `arguments[key]` in a function whose Arguments object codegen
/// elided (`perry-codegen` `codegen/arguments.rs`, `ArgumentsElision::Reads`).
///
/// `raw_args` is the marked Array the caller bundled — the exact input
/// [`js_arguments_object_alloc`] would have copied. An own element (a
/// canonical array index below the bundle's length) reads through the same
/// `clean_arr_ptr` / `js_array_length` / `js_array_get` sequence that alloc
/// uses, so it is the value the object's field would have held. Every other
/// key — `"callee"`, a symbol, a string, a fractional, negative or
/// out-of-range number, an INT32-tagged value, or a hole element — answers
/// `TAG_HOLE`, which no JS read produces, and the caller takes
/// [`js_arguments_bundle_get_slow`]. Cannot allocate, throw, or run user code.
#[no_mangle]
pub extern "C" fn js_arguments_bundle_index_get(raw_args: f64, key: f64) -> f64 {
    let not_own_element = f64::from_bits(crate::value::TAG_HOLE);
    // Only an exact non-negative integer below 2^32 - 1 is an array index. A
    // NaN-boxed key is a NaN, so it fails the range test; -0 names index 0.
    if !(key >= 0.0 && key < u32::MAX as f64) {
        return not_own_element;
    }
    let index = key as u32;
    if index as f64 != key {
        return not_own_element;
    }
    let raw = crate::value::js_nanbox_get_pointer(raw_args) as *const ArrayHeader;
    // Admit the snapshot once. Re-entering clean_arr_ptr and then
    // array_receiver_gc_tag repeated ownership classification for every
    // read, even though both establish the same live Array header fact.
    // Forwarded or descriptor-bearing inputs keep the complete getter below.
    unsafe {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(raw as usize) {
            if header.obj_type == crate::gc::GC_TYPE_ARRAY
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && !crate::array::array_has_element_descriptors_resolved(raw)
                && (*raw).length <= (*raw).capacity
                && (*raw).capacity as usize <= crate::array::array_physical_capacity(raw)
            {
                if index >= (*raw).length {
                    return not_own_element;
                }
                return *(crate::array::array_elements_ptr(raw) as *const f64).add(index as usize);
            }
        }
    }
    let arr = crate::array::clean_arr_ptr(raw);
    if arr.is_null() {
        return not_own_element;
    }
    if index >= crate::array::js_array_length(arr) {
        return not_own_element;
    }
    f64::from_bits(crate::array::js_array_get(arr, index).bits())
}

/// #10509: the cold half of [`js_arguments_bundle_index_get`]. Builds the
/// Arguments object the prologue would have built (`callee_wrapper`, when
/// non-null, is the info of the function whose singleton closure is `callee`) and
/// performs an ordinary `obj[key]` on it, so a non-element key sees the real
/// object's `callee`, `length`, `Symbol.iterator` and `Object.prototype`
/// surface. The object is not kept: codegen only takes this path for a
/// function that never observes the object's identity.
#[no_mangle]
pub extern "C" fn js_arguments_bundle_get_slow(
    raw_args: f64,
    key: f64,
    callee: f64,
    callee_wrapper: *const crate::closure::JsFunctionInfo,
    restricted_callee: i32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_args = scope.root_nanbox_f64(raw_args);
    let key = scope.root_nanbox_f64(key);
    let callee = if callee_wrapper.is_null() {
        callee
    } else {
        let closure = crate::closure::js_closure_alloc_singleton(callee_wrapper);
        crate::value::js_nanbox_pointer(closure as i64)
    };
    let obj = js_arguments_object_alloc(raw_args.get_nanbox_f64(), callee, restricted_callee);
    crate::value::js_dyn_index_get(
        crate::value::js_nanbox_pointer(obj as i64),
        key.get_nanbox_f64(),
    )
}

#[no_mangle]
pub extern "C" fn js_arguments_object_map_index(
    obj: *mut ObjectHeader,
    index: u32,
    box_ptr: *mut crate::r#box::Box,
) {
    if obj.is_null() || box_ptr.is_null() {
        return;
    }
    // Never allocates: the prologue holds `obj` in a bare register across
    // these calls. The room was sized by `js_arguments_object_alloc_mapped`;
    // an index at or past it is one this call did not pass, which
    // CreateMappedArgumentsObject leaves unmapped.
    unsafe {
        let Some(ArgumentsState::Mapped(map)) = arguments_state(obj) else {
            return;
        };
        if index < (*map).length {
            // The cell is a movable heap object, so the element is a traced
            // pointer edge. Store it through the array's resolved slot store:
            // it records the element's layout (a pointer where the array was
            // born all-hole, so the tracer visits the slot) and runs the
            // in-body slot barrier with the ARRAY as parent, so an OLD mapping
            // array storing a YOUNG cell is remembered and the next minor
            // copies the cell and rewrites this element. The mapping array is
            // a private plain `GC_TYPE_ARRAY` born with `length == capacity`
            // covering `index`, so the public setter's exotic-receiver,
            // frozen, sparse and string-addref arms cannot apply; calling the
            // resolved store directly keeps this prologue helper a GC leaf.
            let bits = crate::value::POINTER_TAG | (box_ptr as u64 & crate::value::POINTER_MASK);
            let flags = crate::array::array_object_flags_resolved(map);
            crate::array::store_array_slot_resolved(
                map,
                index as usize,
                f64::from_bits(bits),
                flags,
            );
        }
    }
}

pub(crate) fn is_arguments_object(obj: *const ObjectHeader) -> bool {
    #[cfg(test)]
    TEST_ARGUMENTS_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    // #7854: nothing has ever been created ⟹ nothing can be found. Checked
    // first because it is the only arm that never touches the receiver. See
    // `ARGUMENTS_OBJECTS_EVER_USED`.
    if arguments_registry_never_used() {
        return false;
    }
    if obj.is_null() {
        return false;
    }
    let found = unsafe { arguments_state(obj) }.is_some();
    // The private identity must also revoke ordinary shape-keyed read proofs.
    debug_assert!(
        !found
            || unsafe {
                crate::object::proto_validity::object_is_exotic_read_receiver(obj as usize)
            },
        "arguments identity did not revoke ordinary read proofs"
    );
    found
}

#[cfg(test)]
thread_local! {
/// Every entry into [`is_arguments_object`]. Twin of
/// `set::TEST_SET_REGISTRY_PROBES` — lets a test assert that the latch
/// actually short-circuits rather than merely that nothing threw.
    static TEST_ARGUMENTS_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_arguments_registry_never_used() -> bool {
    arguments_registry_never_used()
}

/// Test-only: drive the process-global latch directly so a test can observe
/// BOTH states deterministically instead of depending on which test in the
/// binary ran first. Callers must restore the previous value.
#[cfg(test)]
pub(crate) fn test_force_arguments_registry_ever_used(value: bool) {
    ARGUMENTS_OBJECTS_EVER_USED.store(value, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_arguments_registry_probe_count() -> u64 {
    TEST_ARGUMENTS_REGISTRY_PROBES.with(|c| c.get())
}

/// Exact birth shape proves all indexed properties remain own data slots.
/// Value stores do not weaken it; deletes, descriptors and prototype edits do.
unsafe fn born_arguments_len(obj: *const ObjectHeader) -> Option<u32> {
    let tail = &*arguments_tail(obj);
    let born = tail.birth_shape;
    (born != LayoutWord(0) && LayoutWord((*obj).shape_word()) == born).then_some(tail.len)
}

unsafe fn born_arguments_index(obj: *const ObjectHeader, index: u32) -> f64 {
    if let Some(cell) = arguments_state(obj).and_then(|state| state.mapped_box(index)) {
        return crate::r#box::js_box_get(cell);
    }
    *(MetaObj::runtime_born(obj as *mut ObjectHeader)
        .slots()
        .cast::<f64>()
        .add(index as usize))
}

unsafe fn born_arguments_length_value(obj: *const ObjectHeader, len: u32) -> f64 {
    *(MetaObj::runtime_born(obj as *mut ObjectHeader)
        .slots()
        .cast::<f64>()
        .add(len as usize))
}

/// Borrow the caller's unchanged list after the exact layout proof. A mapped
/// list must read its cells instead. The borrower enters the callee before
/// anything can collect, and call dispatch roots/spills every supplied value
/// before its first allocation.
pub(crate) unsafe fn arguments_object_born_slice<'a>(
    obj: *const ObjectHeader,
) -> Option<&'a [f64]> {
    arguments_state(obj)?;
    if matches!(arguments_state(obj)?, ArgumentsState::Mapped(_)) {
        return None;
    }
    let born_len = born_arguments_len(obj)?;
    let length = born_arguments_length_value(obj, born_len);
    if !length.is_finite() || length < 0.0 || length > born_len as f64 {
        return None;
    }
    let fields = MetaObj::runtime_born(obj as *mut ObjectHeader)
        .slots()
        .cast::<f64>();
    Some(std::slice::from_raw_parts(fields, length.floor() as usize))
}

pub(crate) unsafe fn arguments_object_get_index(
    obj: *const ObjectHeader,
    index: u32,
) -> Option<f64> {
    if !is_arguments_object(obj) {
        return None;
    }
    if born_arguments_len(obj).is_some_and(|len| index < len) {
        return Some(born_arguments_index(obj, index));
    }
    // Interning the key allocates, and `obj` is dereferenced after it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    let key = intern_key(&index.to_string());
    Some(obj_h.with_const_ptr(|obj| js_object_get_field_by_name_f64(obj, key)))
}

pub(crate) unsafe fn arguments_object_set_index(
    obj: *mut ObjectHeader,
    index: u32,
    value: f64,
) -> bool {
    if !is_arguments_object(obj) {
        return false;
    }
    // Interning the key allocates, and `obj` is dereferenced after it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let key = intern_key(&index.to_string());
    obj_h.with_mut_ptr(|obj| arguments_object_set_field(obj, key, value))
}

pub(crate) fn arguments_object_to_string_tag(value: f64) -> Option<f64> {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
    if !is_arguments_object(ptr) {
        return None;
    }
    let bytes = b"[object Arguments]";
    let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    Some(crate::value::js_nanbox_string(s as i64))
}

pub(crate) unsafe fn arguments_object_get_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let name = key_name(key)?;
    let state = arguments_state(obj)?;
    let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
        if super::own_key_present(obj as *mut ObjectHeader, key) {
            state.mapped_box(idx)
        } else {
            None
        }
    });

    if name == "callee" && state.restricted_callee() {
        arguments_throw_type_error(std::ptr::null(), crate::closure::JsThis::UNDEFINED);
    }
    if let Some(box_ptr) = mapped_box {
        let value = crate::r#box::js_box_get(box_ptr);
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if super::own_key_present(obj as *mut ObjectHeader, key) {
        if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
            if acc.get != 0 {
                let closure =
                    (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                if !closure.is_null() {
                    // An accessor on the arguments object runs with it as `this`.
                    let this = crate::closure::JsThis::from_f64(crate::value::js_nanbox_pointer(
                        obj as i64,
                    ));
                    let value = crate::closure::js_closure_call0(closure, this);
                    return Some(JSValue::from_bits(value.to_bits()));
                }
            }
            return Some(JSValue::undefined());
        }
        return Some(read_ordinary_own_value(obj, key));
    }
    None
}

pub(crate) unsafe fn arguments_object_set_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let Some(name) = key_name(key) else {
        return false;
    };
    let Some(state) = arguments_state(obj) else {
        return false;
    };
    let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
        if super::own_key_present(obj, key) {
            state.mapped_box(idx)
        } else {
            None
        }
    });

    if name == "callee" && state.restricted_callee() {
        arguments_throw_type_error(std::ptr::null(), crate::closure::JsThis::UNDEFINED);
    }
    if !super::own_key_present(obj, key) {
        return false;
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        if acc.set != 0 {
            let closure =
                (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !closure.is_null() {
                let this =
                    crate::closure::JsThis::from_f64(crate::value::js_nanbox_pointer(obj as i64));
                crate::closure::js_closure_call1(closure, this, value);
            }
        }
        return true;
    }
    if let Some(attrs) = get_property_attrs(obj as usize, &name) {
        if !attrs.writable() {
            crate::error::throw_immutable_write(0, &name);
        }
    }
    let mapped = mapped_box.is_some();
    write_ordinary_own_value(obj, key, value);
    // The own-value write can allocate and move the cell: re-derive it.
    if mapped {
        let cell = super::canonical_array_index(&name)
            .and_then(|idx| arguments_state(obj).and_then(|state| state.mapped_box(idx)));
        if let Some(box_ptr) = cell {
            crate::r#box::js_box_set(box_ptr, value);
        }
    }
    true
}

pub(crate) unsafe fn arguments_object_before_delete(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<i32> {
    let name = key_name(key)?;
    let state = arguments_state(obj)?;
    if name == "callee" && state.restricted_callee() {
        return Some(0);
    }
    if let Some(index) = super::canonical_array_index(&name) {
        state.unmap(index);
    }
    None
}

pub(crate) unsafe fn arguments_object_after_define(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    descriptor: &super::object_ops::DescView<'_>,
) {
    let Some(name) = key_name(key) else {
        return;
    };
    let Some(index) = super::canonical_array_index(&name) else {
        return;
    };
    let Some(state) = arguments_state(obj) else {
        return;
    };
    let Some(box_ptr) = state.mapped_box(index) else {
        return;
    };
    if descriptor.has_named(b"value") {
        crate::r#box::js_box_set(
            box_ptr,
            f64::from_bits(descriptor.read_named(b"value").bits()),
        );
    }
    if descriptor.has_named(b"get")
        || descriptor.has_named(b"set")
        || descriptor.flag(b"writable") == Some(false)
    {
        state.unmap(index);
    }
}

pub(crate) unsafe fn arguments_object_descriptor(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let name = key_name(key)?;
    let restricted_callee = arguments_state(obj)?.restricted_callee();
    if !super::own_key_present(obj, key) {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if name == "callee" && restricted_callee {
        let thrower = thrower_closure_value();
        return Some(build_accessor_descriptor(thrower, thrower, false, false));
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        let get = if acc.get != 0 {
            f64::from_bits(acc.get)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let set = if acc.set != 0 {
            f64::from_bits(acc.set)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let attrs =
            get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
        return Some(build_accessor_descriptor(
            get,
            set,
            attrs.enumerable(),
            attrs.configurable(),
        ));
    }
    let value = if let Some(index) = super::canonical_array_index(&name) {
        arguments_state(obj)
            .and_then(|state| state.mapped_box(index))
            .map(|box_ptr| crate::r#box::js_box_get(box_ptr))
            .unwrap_or_else(|| f64::from_bits(read_ordinary_own_value(obj, key).bits()))
    } else {
        f64::from_bits(read_ordinary_own_value(obj, key).bits())
    };
    let attrs =
        get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
    Some(build_data_descriptor(
        value,
        attrs.writable(),
        attrs.enumerable(),
        attrs.configurable(),
    ))
}

pub(crate) unsafe fn arguments_object_to_vec(obj: *const ObjectHeader) -> Option<Vec<f64>> {
    if !is_arguments_object(obj) {
        return None;
    }
    if let Some(born_len) = born_arguments_len(obj) {
        let value = born_arguments_length_value(obj, born_len);
        // Slot VALUE still matters: writing a string length need not change
        // the shape. Non-numbers keep ToLength's full coercion semantics.
        if JSValue::from_bits(value.to_bits()).is_number() {
            let len = if value.is_finite() && value > 0.0 {
                value.floor().min(u32::MAX as f64) as u32
            } else {
                0
            };
            if len <= born_len && !value.is_infinite() {
                return Some((0..len).map(|i| born_arguments_index(obj, i)).collect());
            }
        }
    }
    Some(super::global_this::generic_array_like_to_vec(
        crate::value::js_nanbox_pointer(obj as i64),
    ))
}

/// Typed lowering's fast arm. The selected method VALUE must be the
/// intrinsic apply at this call, or the lookup's by-name answer for a
/// function whose pristine `Function.prototype` holds it; an override returns
/// the internal miss value and codegen invokes that already-selected value
/// through its ordinary site.
#[no_mangle]
pub extern "C-unwind" fn js_arguments_forward_apply(
    method: f64,
    target: f64,
    receiver: f64,
    args: f64,
) -> f64 {
    let bits = method.to_bits();
    if bits == super::method_call::METHOD_CALL_BY_NAME_DIRECT {
        let pair = [receiver, args];
        if let Some(value) = unsafe { super::builtin_method::by_name_function_apply(target, &pair) }
        {
            return value;
        }
    } else if bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        let address = (bits & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(address) {
            if let Some(info) =
                crate::closure::closure_info(address as *const crate::closure::ClosureHeader)
            {
                if info.code == super::global_this::function_prototype_apply_thunk as *const u8 {
                    let pair = [receiver, args];
                    if let Some(value) = unsafe {
                        super::native_call_method::run_function_intrinsic(
                            target,
                            "apply",
                            pair.as_ptr(),
                            2,
                        )
                    } {
                        return value;
                    }
                }
            }
        }
    }
    f64::from_bits(crate::value::TAG_HOLE)
}

/// Typed `f.call(x, ...arguments)`: reuse the selected method value for its
/// cold dispatch. Only the intrinsic call body and the
/// exact unchanged Arguments layout and intrinsic iterator step admit the
/// borrowed-list arm. The existing iterator owner shape proves the step.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_arguments_call_spread(
    method: f64,
    target: f64,
    method_id: i64,
    receiver: f64,
    args: f64,
) -> f64 {
    let bits = method.to_bits();
    let obj = crate::value::js_nanbox_get_pointer(args) as *const ObjectHeader;
    let intrinsic = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
        && crate::closure::closure_info(
            (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader,
        )
        .is_some_and(|info| {
            info.code == super::global_this::function_prototype_call_thunk as *const u8
        });
    let next_owner = super::iterator_prototypes::array_iterator_prototype_addr();
    if intrinsic
        && is_arguments_object(obj)
        && arguments_object_born_slice(obj).is_some()
        && next_owner != 0
        && super::iterator_prototypes::array_iterator_next_is_intrinsic(
            next_owner as *const ObjectHeader,
        )
    {
        // `call` and `apply` enter the same target with the same receiver.
        let pair = [receiver, args];
        if let Some(result) =
            super::native_call_method::run_function_intrinsic(target, "apply", pair.as_ptr(), 2)
        {
            return result;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(method);
    let target = scope.root_nanbox_f64(target);
    let receiver = scope.root_nanbox_f64(receiver);
    let args = scope.root_nanbox_f64(args);
    let mut values = vec![receiver];
    let symbol = crate::symbol::well_known_symbol("iterator");
    let own_iterator = crate::symbol::js_object_has_own_symbol_property(
        args.get_nanbox_f64(),
        crate::value::js_nanbox_pointer(symbol as i64),
    );
    // Ask the actual iterator method on mutated arguments (including a user
    // iterator getter) before falling back to their intrinsic array iterator.
    let iter =
        crate::symbol::get_iterator_from_method(args.get_nanbox_f64()).unwrap_or_else(|| {
            if own_iterator {
                super::throw_object_type_error(b"Arguments iterator is not callable");
            }
            crate::symbol::js_get_iterator(args.get_nanbox_f64())
        });
    let iter = scope.root_nanbox_f64(iter);
    let next = scope.root_nanbox_f64(crate::array::js_iterator_next_method(iter.get_nanbox_f64()));
    loop {
        let mut value = f64::from_bits(crate::value::TAG_UNDEFINED);
        if crate::array::js_iterator_step(iter.get_nanbox_f64(), next.get_nanbox_f64(), &mut value)
            != 0
        {
            break;
        }
        values.push(scope.root_nanbox_f64(value));
    }
    let values = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&values);
    super::method_call::js_method_call_split(
        method.get_nanbox_f64(),
        0,
        target.get_nanbox_f64(),
        method_id,
        values.as_ptr(),
        values.len(),
    )
}

/// Exact shape admits the length SLOT; a mutated or mistyped receiver keeps
/// the ordinary Get. Return the JS value, rather than a ToLength conversion.
#[no_mangle]
pub extern "C-unwind" fn js_arguments_length_value(args: f64) -> f64 {
    let obj = crate::value::js_nanbox_get_pointer(args) as *const ObjectHeader;
    if is_arguments_object(obj) {
        if let Some(len) = unsafe { born_arguments_len(obj) } {
            return unsafe { born_arguments_length_value(obj, len) };
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = scope.root_nanbox_f64(args);
    let key = intern_key("length");
    crate::value::js_dyn_index_get(
        args.get_nanbox_f64(),
        crate::value::js_nanbox_string(key as i64),
    )
}

/// Live `length` read used by the Arguments Array iterator.
pub(crate) unsafe fn arguments_object_length(obj: *const ObjectHeader) -> u32 {
    if !is_arguments_object(obj) {
        return 0;
    }
    let value = js_arguments_length_value(crate::value::js_nanbox_pointer(obj as i64));
    let tagged = JSValue::from_bits(value.to_bits());
    if tagged.is_bigint() {
        super::throw_object_type_error(b"Cannot convert a BigInt value to a number");
    }
    let value = if tagged.is_number() {
        value
    } else {
        crate::builtins::js_number_coerce(value)
    };
    if !value.is_nan() && value > 0.0 {
        value.floor().min(u32::MAX as f64) as u32
    } else {
        0
    }
}

/// Live indexed read used by the Arguments Array iterator.
pub(crate) unsafe fn arguments_object_index_value(obj: *const ObjectHeader, index: u32) -> f64 {
    if !is_arguments_object(obj) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    arguments_object_get_index(obj, index)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) unsafe fn arguments_object_to_array(
    obj: *const ObjectHeader,
) -> Option<*mut ArrayHeader> {
    if !is_arguments_object(obj) {
        return None;
    }
    // Every read below interns its key and may run an accessor, so the source
    // and the result are rooted, and each value lands in the result before
    // the next allocation. The result is born with room for every element, so
    // no push grows it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    let len = obj_h.with_const_ptr(|obj| arguments_object_length(obj));
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(len));
    for i in 0..len {
        let value = obj_h.with_const_ptr(|obj| arguments_object_index_value(obj, i));
        let next = out.with_mut_ptr(|arr| crate::array::js_array_push_f64(arr, value));
        out.set_raw_mut_ptr(next);
    }
    Some(out.with_mut_ptr(|arr: *mut ArrayHeader| arr))
}

#[no_mangle]
pub extern "C" fn js_array_like_to_array(value: f64) -> *mut ArrayHeader {
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return std::ptr::null_mut();
    }
    let raw = jsv.as_pointer::<u8>();
    unsafe {
        if let Some(arr) = arguments_object_to_array(raw as *const ObjectHeader) {
            return arr;
        }
        let addr = raw as usize;
        if crate::typedarray_hook_slots::lookup_typed_array_kind(addr).is_some() {
            return crate::typedarray_hook_slots::typed_array_to_array(
                raw as *const crate::typedarray_hook_slots::TypedArrayHeader,
            );
        }
        if crate::buffer::is_registered_buffer(addr) {
            return crate::buffer::buffer_to_array(raw as *const crate::buffer::BufferHeader);
        }
        // A real Array → the dominant plain shape needs only the sticky
        // iterator-protocol check and one receiver-shape compare. An own key
        // (including Symbol.iterator) or a custom prototype leaves that shape;
        // only then pay the complete dense-spread proof (#11772).
        if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
            if let Some(arr) = crate::array::plain_call_spread_source(value) {
                return arr as *mut ArrayHeader;
            }
            if let Some(arr) = crate::array::dense_spread_source(value) {
                return arr as *mut ArrayHeader;
            }
            return crate::array::js_array_clone_for_spread(value);
        }
        // Generic iterable with a user `[Symbol.iterator]` (Map/Set, generator
        // objects, hand-rolled iterables, …): spread uses the ITERATOR protocol
        // (GetIterator → IteratorStep → IteratorValue), NOT array-like
        // length/index access. Drive `.next()` to collect the yielded values so
        // `f(...iterable)` / `new C(...iterable)` / `[...iterable]` spread see
        // them, and so errors thrown by `[Symbol.iterator]` / `.next()` /
        // accessing `value`/`done` propagate (test262 spread-err-*). Plain
        // array-like objects (a `{length, 0, 1}` bag with no `@@iterator`) keep
        // the legacy reinterpret fallback below.
        if crate::collection_iter::is_iterable(value) {
            let iter = crate::symbol::js_get_iterator(value);
            let mut out = crate::array::js_array_alloc(0);
            while let Some(v) = crate::collection_iter::iterator_next_value(iter) {
                out = crate::array::js_array_push_f64(out, f64::from_bits(v.to_bits()));
            }
            return out;
        }
        crate::array::clean_arr_ptr(raw as *const ArrayHeader) as *mut ArrayHeader
    }
}

unsafe fn read_ordinary_own_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let key_count = keys_view.count() as usize;
    let alloc_limit = std::cmp::max(
        crate::object::object_live_slot_count(obj),
        crate::object::INLINE_SLOT_FLOOR as u32,
    ) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            let i = keys_view.storage_slot(i as u32) as usize;
            if i < alloc_limit {
                return js_object_get_field(obj, i as u32);
            }
            return overflow_get(obj as usize, i)
                .map(JSValue::from_bits)
                .unwrap_or_else(JSValue::undefined);
        }
    }
    JSValue::undefined()
}

unsafe fn write_ordinary_own_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let key_count = keys_view.count() as usize;
    let alloc_limit = std::cmp::max(
        crate::object::object_live_slot_count(obj),
        crate::object::INLINE_SLOT_FLOOR as u32,
    ) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            let i = keys_view.storage_slot(i as u32) as usize;
            if i < alloc_limit {
                js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
            } else {
                overflow_set(obj as usize, i, value.to_bits());
            }
            return;
        }
    }
}

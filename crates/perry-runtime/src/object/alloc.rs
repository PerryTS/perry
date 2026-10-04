//! Object allocation: `js_object_alloc*`, class-keys array builders,
//! shape-cache-backed fast paths, and the clone/copy helpers (`Object.assign`
//! lives in `object/assign.rs`).
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes. Shared state and helpers remain in the parent `object`
//! module and are reached via `use super::*;`.

pub use super::alloc_basic::{
    js_object_alloc, js_object_alloc_fast, js_object_alloc_fast_with_parent,
    js_object_alloc_with_parent, js_object_coerce,
};
use super::*;

// Storage: `ObjectHotTables::class_keys_by_id` (this agent's class_id ->
// (keys array address, field count) memo). See its field docs for why it is
// per agent and weak.

fn remember_class_keys(class_id: u32, field_count: u32, keys: crate::object::ObjectKeys) {
    if class_id == 0 || keys.is_null() {
        return;
    }
    crate::state::state()
        .object_hot
        .class_keys_by_id
        .borrow_mut()
        .insert(class_id, (keys.arr() as usize, field_count, keys.count()));
    // #6759 C5a: harvest this class's declared instance-field names into
    // the process-wide name-hash set the per-key inline-guard vetting
    // consults — and retro-check them against prototype-level descriptor
    // keys installed BEFORE this class registered (module-init ordering
    // must not create an unsound skip).
    unsafe {
        let count = field_count.min(keys.count()) as usize;
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..count {
            let v = keys.get(i as u32);
            if let Some(b) = crate::string::js_string_key_bytes(v, &mut sso) {
                super::descriptor_state::note_declared_instance_field_name(b);
            }
        }
    }
}

/// GC root scanner: rewrite each remembered keys-array address across a move.
/// WEAK — the address is visited as metadata, never marked, so remembering a
/// class's keys array does not keep it alive.
pub fn scan_class_keys_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let st = crate::state::state();
    let mut map = st.object_hot.class_keys_by_id.borrow_mut();
    for entry in map.values_mut() {
        if entry.0 == 0 {
            continue;
        }
        let mut addr = entry.0;
        if visitor.visit_metadata_usize_slot(&mut addr) {
            entry.0 = addr;
        }
    }
}

/// Post-trace death prune. A dropped entry costs one rebuild of the class's
/// keys array — `registered_class_keys_array` already answers `None` for a
/// zeroed address and every caller re-derives — which is what makes
/// weakening this table safe rather than merely possible.
///
/// The table is this agent's, so every address in it belongs to this agent's
/// heap: an address the header probe cannot attribute has been recycled, not
/// borrowed from another thread, and is dropped — the same `None` arm the
/// canonical trie uses (`canonical_keys::canonical_address_is_recycled`).
#[cold]
pub(crate) fn prune_dead_class_keys_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    let st = crate::state::state();
    let mut map = st.object_hot.class_keys_by_id.borrow_mut();
    map.retain(|_, entry| {
        let addr = entry.0;
        if addr == 0 {
            return false;
        }
        if is_dead_owner(addr) {
            return false;
        }
        // An address the arena recycled for a non-array tenant is dead to us
        // whatever `is_dead_owner` says about the new occupant.
        // SAFETY: a read-only tracked-header probe.
        unsafe {
            match crate::value::addr_class::try_read_tracked_gc_header(addr) {
                Some(gc) => {
                    let ty = (*gc.as_ptr()).obj_type;
                    ty == crate::gc::GC_TYPE_ARRAY || ty == crate::gc::GC_TYPE_LAZY_ARRAY
                }
                None => false,
            }
        }
    });
}

/// This agent's memoized keys array for `class_id`. The address is current
/// only until the next allocation: a caller that allocates before its last
/// use must root it or read this again after the allocation.
pub(crate) fn registered_class_keys_array(
    class_id: u32,
) -> Option<(crate::object::ObjectKeys, u32)> {
    let (addr, field_count, key_count) = crate::state::state()
        .object_hot
        .class_keys_by_id
        .borrow()
        .get(&class_id)
        .copied()?;
    if addr == 0 {
        return None;
    }
    Some((
        crate::object::ObjectKeys::new(addr as *mut ArrayHeader, key_count),
        field_count,
    ))
}

/// #1175: allocate an object whose `[[Prototype]]` is null. Same layout as
/// `js_object_alloc`, but the `OBJ_FLAG_NULL_PROTO` bit is set on the GC
/// header so `Object.getPrototypeOf` returns null instead of the heap
/// pointer / synthesized proto. Used by `querystring.parse` to mirror Node's
/// `Object.create(null)`-backed result and dodge prototype-pollution
/// surprises.
#[no_mangle]
pub extern "C" fn js_object_alloc_null_proto(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let ptr = js_object_alloc_with_parent(class_id, 0, field_count);
    unsafe {
        let gc = (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
    }
    ptr
}

/// Allocate a class instance's storage while the caller holds `keys` — a keys
/// array it received as a raw copy of a root it does not own (a codegen
/// per-class global, the class memo, the shape cache) — and hand back both the
/// storage and the keys array's address AFTER the allocation.
///
/// #10969 review, finding 2: a canonical keys array is an ordinary movable
/// allocation, so a pointer read before a collecting allocation names
/// from-space after it. The open-block bump cannot collect
/// (`arena_alloc_gc_no_collect`), so the common case uses `keys` as received
/// and pays nothing; only the block-exhausted path, which is also the only
/// path that can collect, roots `keys` across the allocation and reloads it.
#[inline(always)]
fn alloc_instance_keeping_keys(
    total_size: usize,
    keys: *mut ArrayHeader,
) -> (*mut ObjectHeader, *mut ArrayHeader) {
    let raw = crate::arena::arena_alloc_gc_no_collect(total_size, 8, crate::gc::GC_TYPE_OBJECT);
    if !raw.is_null() {
        return (raw as *mut ObjectHeader, keys);
    }
    alloc_instance_keeping_keys_collecting(total_size, keys)
}

#[cold]
#[inline(never)]
fn alloc_instance_keeping_keys_collecting(
    total_size: usize,
    keys: *mut ArrayHeader,
) -> (*mut ObjectHeader, *mut ArrayHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_handle = scope.root_raw_mut_ptr(keys);
    keys_handle.across_mut::<ArrayHeader, _>(|| {
        arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader
    })
}

/// Build a fresh longlived keys array: `prefix[0..prefix_len]` (the dynamic
/// parent's keys, or nothing) followed by one longlived string per `keys`.
///
/// Every allocation here can collect — the longlived arena reaches
/// `gc_check_trigger` when its block is full — so the array under
/// construction and `prefix` are both held in handles and reloaded after each
/// allocation, and the prefix is copied last, after the final allocation. The
/// slots are cleared right after the array is born so a collection that
/// traces the unfinished array never reads uninitialized words. Callers own
/// the layout policy (`js_build_class_keys_array` adds its immortal scope).
///
/// Every caller passes key names from program text: the key literals the
/// modules' string pools mint as ATOMS (`js_string_pool_atom`), the one
/// string object per key text a read site passes and a canonical list
/// stores. A list can be built before the pool holding one of its texts runs
/// (a seed runs before every pool; a class registers at its own module's
/// init, before the modules it does not import), so the atom of each name is
/// minted HERE first (the pool finds it later) and the canonical copy stores
/// the atoms, exactly as a list first written after the pools ran. Without
/// it the list holds strings no read site ever passes, and every pointer
/// confirm against it (the megamorphic slot guess) misses.
///
/// # Safety
/// `prefix` is a live keys array with at least `prefix_len` slots, or null
/// with `prefix_len == 0`, and was read with no allocation since.
pub(crate) unsafe fn build_longlived_keys_array(
    prefix: *mut ArrayHeader,
    prefix_len: u32,
    keys: &[&[u8]],
) -> *mut ArrayHeader {
    let total = prefix_len as usize + keys.len();
    let scope = crate::gc::RuntimeHandleScope::new();
    let prefix_handle = scope.root_raw_mut_ptr(prefix);
    let (arr, _) = prefix_handle.across_mut::<ArrayHeader, _>(|| {
        for key_bytes in keys {
            mint_pool_atom(key_bytes);
        }
        crate::array::js_array_alloc_with_length_longlived(total as u32)
    });
    let slots = crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut u64;
    for i in 0..total {
        // GC_STORE_AUDIT(POINTER_FREE): clearing the unfinished array's slots.
        slots.add(i).write(crate::value::TAG_UNDEFINED);
    }
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let mut arr = arr;
    for (j, key_bytes) in keys.iter().enumerate() {
        let (str_ptr, reloaded) = arr_handle.across_mut::<ArrayHeader, _>(|| {
            crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(),
                key_bytes.len() as u32,
            )
        });
        arr = reloaded;
        let bits = crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK);
        let idx = prefix_len as usize + j;
        // GC_STORE_AUDIT(BARRIERED): keys-array slot is reflected into layout metadata.
        *(crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut u64).add(idx) = bits;
        crate::array::note_array_slot_layout_only(arr, idx, bits);
    }
    // No allocation from here on: `arr` is the address after the last one,
    // and the prefix is read from its handle.
    if prefix_len > 0 {
        prefix_handle.with_const_ptr(|prefix: *const ArrayHeader| {
            let src = crate::array::array_elements_ptr(prefix) as *const u64;
            let dst = crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut u64;
            for i in 0..prefix_len as usize {
                let bits = *src.add(i);
                // GC_STORE_AUDIT(INIT): parent key copied into the unpublished array.
                *dst.add(i) = bits;
                crate::array::note_array_slot_layout_only(arr, i, bits);
            }
        });
    }
    arr
}

/// Mint (or find) the atom a module pool mints for key literal `name`: a
/// pool gives one to every non-empty UTF-8 literal of at most
/// `INTERN_MAX_BYTE_LEN` bytes (a WTF-8 literal holds a lone surrogate, is
/// not UTF-8, and gets none). Nothing is held across the allocation; the
/// atom table roots the atom.
fn mint_pool_atom(name: &[u8]) {
    if name.is_empty()
        || name.len() > crate::string::INTERN_MAX_BYTE_LEN as usize
        || std::str::from_utf8(name).is_err()
    {
        return;
    }
    let hash = super::key_bytes_hash(name.as_ptr(), name.len());
    crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
}

/// Fast class instance allocator that takes a pre-built keys_array
/// pointer directly, skipping the per-call SHAPE_CACHE lookup. The
/// codegen pre-builds the keys_array ONCE at module init time
/// (via `js_build_class_keys_array`) and stores the result in a
/// per-class global, then passes that global to this allocator on
/// every `new ClassName()` call. This eliminates the thread-local
/// + RefCell::borrow_mut + HashMap::get cost from the hot
/// allocation path — for benchmarks like `object_create` (1M
/// `new Point(...)` calls) the SHAPE_CACHE lookup was ~30ns/alloc.
///
/// `#[inline]` lets the bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) inline the entire body — including
/// the `arena_alloc_gc` call — into the user's `new ClassName()`
/// site, eliminating function-call overhead from the hot loop.
#[inline]
/// Returns the header plus the BIRTH live inline-slot bound the allocation was
/// sized for. #8113: the header no longer carries a `field_count` word, so the
/// widened bound this computes has to travel back to the caller that stamps it.
/// The last element is `keys_array`'s address after the allocation, which is
/// the only one a caller may use.
pub(super) fn object_alloc_class_inline_keys_impl(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys: crate::object::ObjectKeys,
    preinstalled_shape_id: u32,
    premark_plain: bool,
) -> (*mut ObjectHeader, u32, bool, crate::object::ObjectKeys) {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
    let header_size = std::mem::size_of::<ObjectHeader>();
    // #6812 (w16): honor the learned high-water width for this class so
    // builder-pattern instances allocate their true field count inline
    // instead of spilling writes to the overflow side-table. The stored
    // field_count must be the widened count too — read/write paths derive
    // alloc_limit as max(field_count, INLINE_SLOT_FLOOR) — mirroring the
    // dynamic-construct path, which already passes
    // `learned_inline_field_count` as the field count (capacity semantics;
    // enumeration follows keys_array, not field_count).
    let learned = crate::object::learned_inline_field_count(class_id) as usize;
    let logical_field_count = std::cmp::max(field_count as usize, learned);
    let alloc_field_count = std::cmp::max(logical_field_count, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    // `keys` names a raw copy of the caller's root and is not nameable past
    // this call; the helper hands back its post-allocation address.
    let (ptr, keys_array) = alloc_instance_keeping_keys(total_size, keys.arr());
    let keys = crate::object::ObjectKeys::new(keys_array, keys.count());

    let used_preinstalled_shape = unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        if premark_plain {
            // Charter step 3: marked before the first stamp, so the birth
            // shape is minted `Ordinary` and no twin is ever needed.
            crate::object::shapes::store_kind::premark_plain_ordinary(ptr);
        }
        // The compiled entry point passes the ShapeId installed beside this
        // canonical keys global at module initialization. Reuse that immutable
        // descriptor directly when its keys facts and live-slot bound still
        // match; learned instance widening, key-count drift, and worker-local
        // first installs retain the exact mint-and-validate fallback.
        let used_preinstalled_shape = preinstalled_shape_id != 0
            && crate::object::shapes::try_birth_stamp_preinstalled_shape(
                ptr,
                preinstalled_shape_id,
                keys,
                logical_field_count as u32,
            );
        if !used_preinstalled_shape {
            // #8113: the birth live-slot bound is a PARAMETER now — it used to
            // be read back out of the `(*ptr).field_count` store that stood
            // here.
            set_object_keys_with_live(ptr, keys, logical_field_count as u32);
        }

        // PerryTS/perry#4717: initialize ALL `max(field_count, 8)` field slots to
        // `undefined`, mirroring `js_object_alloc_with_parent`. The arena hands back
        // recycled bytes, so without this a field read-before-write — or a GC that
        // scans the still-constructing instance — would observe stale arena bytes
        // from a previously-freed object (e.g. `marked`'s `this.defaults` crashing
        // with "Cannot read properties of undefined"). This used to be the caller's
        // job (the inline bump path and `json/parser.rs` both zero-filled by hand);
        // folding it in here keeps every caller — including the outlined `new C()`
        // codegen path — correct by construction.
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot initialized to undefined.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        used_preinstalled_shape
    };
    (
        ptr,
        logical_field_count as u32,
        used_preinstalled_shape,
        keys,
    )
}

/// Compatibility entry point for runtime callers that do not have a
/// module-init ShapeId.
///
/// It mints the id from the canonical keys array instead of receiving it, so
/// the instance is still stamped AT BIRTH. Leaving it to rung 1's lazy
/// self-heal would split this class's population between stamped and newborn
/// receivers, which the emitted PIC cannot tolerate — see
/// `shapes::birth_stamp_object_shape`. The mint is one shape-table probe and
/// this is not the compiled hot path (compiled `new C(…)` sites call
/// `js_object_alloc_class_inline_keys_stamped` with a module-init id).
///
/// The C entry takes a bare array, so it can only be handed an array whose
/// header length IS the list (an exclusively owned or exact one). Runtime
/// callers that hold a class's keys as a view — the class memo, a JSON shape
/// hint — call [`alloc_class_instance_with_keys`] with the view's count.
#[no_mangle]
pub extern "C" fn js_object_alloc_class_inline_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
) -> *mut ObjectHeader {
    let keys = unsafe { crate::object::ObjectKeys::owned(keys_array) };
    alloc_class_instance_with_keys(class_id, parent_class_id, field_count, keys)
}

/// [`js_object_alloc_class_inline_keys`] for a caller that holds the class's
/// keys as a view, count included.
pub(crate) fn alloc_class_instance_with_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys: crate::object::ObjectKeys,
) -> *mut ObjectHeader {
    super::alloc_plain::alloc_class_instance_with_keys_impl(
        class_id,
        parent_class_id,
        field_count,
        keys,
        false,
    )
}

/// The compiled-class allocation entry point after #6759 C3 rung 2.
///
/// `shape_id` is minted once from the same canonical `keys_array` at module
/// initialization. Installing it after the existing allocator returns keeps
/// every allocation/rooting/layout invariant above in one implementation,
/// while making a fresh class instance immediately usable by ShapeId guards.
/// ShapeId exhaustion fail-stops during module initialization; no newborn can
/// be published with a pointer/count fallback identity.
#[no_mangle]
pub extern "C" fn js_object_alloc_class_inline_keys_stamped(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
    shape_id: u32,
    rep: u64,
) -> *mut ObjectHeader {
    // The key count comes from the shape the module-init code minted beside
    // this keys global: the global holds only the array, and the array can
    // be a canonical backing longer than this class's list.
    super::alloc_plain::alloc_class_inline_keys_stamped_impl(
        class_id,
        parent_class_id,
        field_count,
        keys_array,
        shape_id,
        rep,
        false,
    )
}

/// Build (or fetch from SHAPE_CACHE) the keys_array for a class.
/// Called ONCE per class at module init time; the resulting pointer
/// is cached in a per-class global by the codegen and then passed
/// to `js_object_alloc_class_inline_keys` on each `new` call.
///
/// Same packed-keys format as `js_object_alloc_class_with_keys`:
/// null-separated UTF-8 field names.
///
/// `rep` is the class's birth rep (charter step 5): the shape minted beside
/// the keys carries it, so an allocator that births from this cache entry
/// (`js_object_alloc_class_with_keys`, the runtime construct path through
/// `alloc_plain::class_keys_birth_rep`) gets the birth rep, never an all-`Any`
/// twin of the class's shape.
#[no_mangle]
pub extern "C" fn js_build_class_keys_array(
    class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
    rep: u64,
) -> *mut ArrayHeader {
    let shape_id = super::alloc_plain::class_keys_cache_slot(class_id, field_count);
    let cached = shape_cache_get(shape_id);
    if !cached.is_null() {
        remember_class_keys(class_id, field_count, cached);
        return cached.arr();
    }
    if field_count == 0 || packed_keys_len == 0 || packed_keys.is_null() {
        let arr = crate::array::js_array_alloc_with_length_longlived(0);
        let (_, keys) = shape_cache_insert(
            shape_id,
            crate::object::canonical_keys::LiveObject::none(),
            crate::object::ObjectKeys::new(arr, 0),
            rep,
        );
        remember_class_keys(class_id, field_count, keys);
        return keys.arr();
    }
    let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
    let keys: Vec<&[u8]> = crate::object::packed_key_names(keys_bytes);
    // This array is long-lived and never dies. Without the scope, the per-slot
    // notes in the builder mint a per-object pointer mask for any class with
    // enough keys, which arms `PERRY_PER_OBJECT_LAYOUTS_ANY` and puts the
    // address filter probe on EVERY later allocation in the program (measured
    // as 3% of an allocation-heavy ECS row: `layout_forget_object` from each
    // object literal). Under the scope the notes settle on the tag-checked
    // scan, and `layout_init_all_pointer_slots` below records the final
    // all-pointer layout anyway.
    let _immortal = crate::gc::ImmortalLayoutScope::new();
    // Issue #179: route the array and its key strings through the longlived
    // arena so general-arena block 0 doesn't get pinned by the first
    // `new C()` in a loop, which cascaded via block-persistence into every
    // subsequent iteration's allocations.
    let arr = unsafe { build_longlived_keys_array(ptr::null_mut(), 0, &keys) };
    // #7510: every slot in `0..length` now holds an interned key string, and a
    // canonical keys array is immutable for the rest of the program (growing a
    // shape builds a NEW array — `shape_keys_grown`). Say that in the header
    // instead of leaving the per-element pointer mask behind.
    //
    // The mask is correct but permanent: the shape cache anchors this array for
    // the program's lifetime (#179), so its `LAYOUT_SLOT_MASKS` entry never
    // drains. One such entry is enough to keep the whole per-object side table
    // non-empty — and every probe of it on the allocation, store, death and
    // trace paths then has to hash instead of taking the emptiness fast path.
    // Since ~every program builds at least one shape, that made the fast path
    // essentially dead: on `churn_alloc` it fired once in 40 million calls.
    //
    // The per-element notes in the builder stay. They are what keeps the
    // already-stored prefix traceable if allocating the *next* key string
    // triggers a GC; the declaration can only be made once the last slot is
    // filled, which is here.
    unsafe {
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
    }
    // The builder's array is exclusively owned until `shape_cache_insert`
    // canonicalizes it.
    let (_, keys) = shape_cache_insert(
        shape_id,
        crate::object::canonical_keys::LiveObject::none(),
        unsafe { crate::object::ObjectKeys::owned(arr) },
        rep,
    );
    remember_class_keys(class_id, field_count, keys);
    // Generated code keeps only the array; `js_object_alloc_class_inline_keys_stamped`
    // recovers the count from the ShapeId minted beside it.
    keys.arr()
}

/// Allocate a class instance with a shape-cached keys array for field names.
/// This allows dynamic property access (obj.field1) to work on class instances,
/// not just object literals. Uses class_id as the shape_id for caching.
///
/// Marked `#[inline]` so the LLVM bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) can inline the body into hot
/// allocation loops, eliminating the function-call overhead and
/// letting LLVM constant-fold the SHAPE_INLINE_CACHE slot index when
/// `class_id` is a compile-time constant (which it always is at the
/// `new ClassName()` call site).
#[no_mangle]
pub extern "C" fn js_object_alloc_class_with_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    // Register parent class if needed
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    // Use class_id as shape_id for caching the keys array.
    // Hot path: direct-mapped inline cache lookup (no RefCell, no
    // HashMap). Miss path: lazy-build from packed_keys.
    //
    // The keys are resolved BEFORE the instance exists. A miss allocates —
    // the array, its key strings, and the canonical copy `shape_cache_insert`
    // makes — and an instance allocated first would have to be carried across
    // all of them (it used to be carried raw across the first two, which can
    // collect).
    let shape_id = super::alloc_plain::class_keys_cache_slot(class_id, field_count);
    let (cached, cached_runtime_id) = shape_cache_get_with_id(shape_id);
    let (keys_arr, runtime_shape_id) = if !cached.is_null() {
        (cached, cached_runtime_id)
    } else {
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = crate::object::packed_key_names(keys_bytes);
        // Issue #179: shape-cache keys_array lives in the longlived arena
        // (see `js_build_class_keys_array` for the rationale).
        let arr = unsafe { build_longlived_keys_array(ptr::null_mut(), 0, &keys) };
        let (_, keys) = shape_cache_insert(
            shape_id,
            crate::object::canonical_keys::LiveObject::none(),
            unsafe { crate::object::ObjectKeys::owned(arr) },
            super::field_rep::REP_ANY,
        );
        (keys, shape_cache_get_with_id(shape_id).1)
    };
    // The entry's shape carries the class's birth rep (a module-init entry,
    // `js_build_class_keys_array`); every birth from it carries that rep too.
    let rep = super::alloc_plain::shape_rep_of(runtime_shape_id);

    let (ptr, arr) = alloc_instance_keeping_keys(total_size, keys_arr.arr());
    let keys_arr = crate::object::ObjectKeys::new(arr, keys_arr.count());
    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        set_object_keys_with_live(ptr, keys_arr, field_count);
        // #6759 C3 rung 2, completed: birth-stamp here too. #8009 stamped the
        // COMPILED entry point (`js_object_alloc_class_inline_keys_stamped`)
        // and left this one lazily self-healing, which is a SPLIT population
        // for every class that lands here — and a split population is a
        // permanent PIC miss, not a slow start. See
        // `shapes::birth_stamp_object_shape`.
        crate::object::shapes::birth_stamp_object_shape(ptr, runtime_shape_id, field_count, rep);
        if rep != super::field_rep::REP_ANY {
            super::field_rep_store::birth_fill_f64_lanes(ptr);
        }
    }
    remember_class_keys(class_id, field_count, keys_arr);
    ptr
}

/// Allocate a subclass instance whose parent was resolved DYNAMICALLY at
/// runtime — the `class X extends _mod.default` interop-ESM shape (wall 38).
///
/// At X's compile time the parent's field layout is unknown (the `extends`
/// target is an unresolvable cross-module value, so X's `extends_name` is the
/// unresolved `"default"` and `class_field_global_index`'s parent walk bails),
/// so codegen can only size the instance for X's OWN fields. That
/// under-allocates and mis-lays-out the instance: the parent's constructor (run
/// on this `this` via `run_class_constructor_on_this_flat`) and the parent's
/// inherited methods both address the inherited `__perry_cap_*` / declared
/// fields at the PARENT's own slot indices (parent fields come first in the
/// layout), which lie past X's own-only slots → out-of-bounds reads/writes into
/// adjacent heap. That is wall 45 (`Derived extends _base.default` reads
/// `_c10`/`_c20` captures as garbage numbers/functions).
///
/// The parent edge (`js_register_class_parent_dynamic`) and the parent's
/// keys-array (`js_build_class_keys_array`) are both registered at module-init
/// time, before any `new X()`. So here — at construction time — resolve them and
/// allocate with the MERGED layout: `field_count = parent_field_count +
/// own_field_count` and `keys_array = [parent keys..] ++ [own keys..]` (parent
/// first, exactly the slot order the parent's compiled methods/ctor expect).
/// The parent's keys-array already encodes its WHOLE chain (it was built
/// parent-first at the parent's own compile time, where its ancestors were
/// known), so the immediate parent's registered keys are sufficient. Falls back
/// to the own-only layout (`js_object_alloc_class_with_keys`) when no dynamic
/// parent / parent keys are registered (e.g. the parent is a builtin or a
/// not-yet-initialized module).
#[no_mangle]
pub extern "C" fn js_object_alloc_class_dynamic_parent(
    class_id: u32,
    own_field_count: u32,
    own_packed_keys: *const u8,
    own_packed_keys_len: u32,
) -> *mut ObjectHeader {
    let parent_cid = crate::object::get_parent_class_id(class_id).unwrap_or(0);
    let parent_keys = if parent_cid != 0 {
        registered_class_keys_array(parent_cid)
    } else {
        None
    };
    let Some((parent_keys, _parent_fc)) = parent_keys else {
        // No dynamic parent layout available — own-only fallback keeps the
        // prior baseline (correct for parentless / builtin-parent classes).
        return js_object_alloc_class_with_keys(
            class_id,
            parent_cid,
            own_field_count,
            own_packed_keys,
            own_packed_keys_len,
        );
    };
    let parent_arr = parent_keys.arr();
    let parent_len = parent_keys.count();

    // Cache the merged keys-array per class. The shape id is namespaced away
    // from the own-only shape (`+ 2_000_000`) so it can't collide with the
    // `js_build_class_keys_array` / `js_object_alloc_class_with_keys` shapes.
    let shape_id = class_id.wrapping_mul(10007).wrapping_add(2_000_000);
    let (cached, cached_runtime_id) = shape_cache_get_with_id(shape_id);
    let (merged_arr, field_count, runtime_shape_id) = if !cached.is_null() {
        (cached, cached.count(), cached_runtime_id)
    } else {
        let own_keys: Vec<&[u8]> = if own_packed_keys.is_null() || own_packed_keys_len == 0 {
            Vec::new()
        } else {
            let bytes = unsafe {
                std::slice::from_raw_parts(own_packed_keys, own_packed_keys_len as usize)
            };
            crate::object::packed_key_names(bytes)
        };
        let merged_len = parent_len as usize + own_keys.len();
        // `parent_arr` was read from the memo with no allocation since; the
        // builder roots it across its own allocations and copies it last.
        let arr = unsafe { build_longlived_keys_array(parent_arr, parent_len, &own_keys) };
        // No object exists yet, so nothing unrooted crosses this call.
        let (_, merged) = shape_cache_insert(
            shape_id,
            crate::object::canonical_keys::LiveObject::none(),
            unsafe { crate::object::ObjectKeys::owned(arr) },
            super::field_rep::REP_ANY,
        );
        debug_assert_eq!(merged.count() as usize, merged_len);
        (
            merged,
            merged_len as u32,
            shape_cache_get_with_id(shape_id).1,
        )
    };

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;
    let (ptr, arr) = alloc_instance_keeping_keys(total_size, merged_arr.arr());
    let merged_arr = crate::object::ObjectKeys::new(arr, merged_arr.count());
    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_cid;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        set_object_keys_with_live(ptr, merged_arr, field_count);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // The dynamically-parented subclass shape needs the same birth stamp
        // as every other class instance, or its sites split the same way.
        let rep = super::field_rep::REP_ANY;
        crate::object::shapes::birth_stamp_object_shape(ptr, runtime_shape_id, field_count, rep);
    }
    remember_class_keys(class_id, field_count, merged_arr);
    ptr
}

/// Keepalive anchor — `js_object_alloc_class_dynamic_parent` is a
/// generated-code-only callee, so the auto-optimize whole-program build would
/// otherwise dead-strip it (see the FFI-symbol-link-break class).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_OBJECT_ALLOC_CLASS_DYNAMIC_PARENT: extern "C" fn(
    u32,
    u32,
    *const u8,
    u32,
) -> *mut ObjectHeader = js_object_alloc_class_dynamic_parent;

/// Allocate an object with a shape-cached keys array.
/// First call per shape_id creates the keys array from packed_keys (null-separated key names);
/// subsequent calls reuse the cached pointer. This eliminates per-object key string allocation
/// and array construction for repeated object literals with the same shape.
#[no_mangle]
pub extern "C" fn js_object_alloc_with_shape(
    shape_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate extra field slots for dynamic property growth (plain objects may get new fields)
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * 8;
    let total_size = header_size + fields_size;
    let obj_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*obj_ptr).class_id = 0;
        (*obj_ptr).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*obj_ptr).meta = ptr::null_mut();

        // Initialize all allocated field slots to undefined (including extra padding)
        let fields_ptr = (obj_ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(obj_ptr as *mut u8);
    }

    // A cache miss below allocates the keys array and every key string. Keep
    // the newborn object live and reload it before installing the finished
    // shape; otherwise a moving collection leaves `obj_ptr` in from-space.
    let obj_scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = obj_scope.root_raw_mut_ptr(obj_ptr);
    let (cached, cached_runtime_id) = shape_cache_get_with_id(shape_id);
    let (keys_arr, runtime_shape_id) = if !cached.is_null() {
        (cached, cached_runtime_id)
    } else {
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = crate::object::packed_key_names(keys_bytes);
        // Issue #179: shape-cache keys_array lives in the longlived arena.
        // The builder roots the unfinished array across its key allocations;
        // the object is already held in `obj_scope`.
        let arr = unsafe { build_longlived_keys_array(ptr::null_mut(), 0, &keys) };
        // No unrooted receiver crosses this call: the object is held in
        // `obj_scope` and reloaded below.
        let (_, keys) = shape_cache_insert(
            shape_id,
            crate::object::canonical_keys::LiveObject::none(),
            unsafe { crate::object::ObjectKeys::owned(arr) },
            super::field_rep::REP_ANY,
        );
        (keys, shape_cache_get_with_id(shape_id).1)
    };

    unsafe {
        let obj_ptr = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        // A shape-cache HIT hands back the canonical keys array paired with
        // its already-minted ShapeId, so stamp the newborn straight from that
        // immutable descriptor — the same `try_birth_stamp_preinstalled_shape`
        // the compiled-class allocator uses — instead of re-canonicalizing
        // the identical facts on every birth. The publish-then-stamp path
        // below (`publish_object_shape_from` hashing `ShapeFacts`, plus three
        // descriptor lookups in `birth_stamp_object_shape`) was the bulk of a
        // 257 ns `{ a, b, m() {} }` literal; a miss (first birth of the shape,
        // worker-local id not yet installed, bound mismatch) keeps it.
        let stamped_from_cache = runtime_shape_id != 0
            && crate::object::shapes::try_birth_stamp_preinstalled_shape(
                obj_ptr,
                runtime_shape_id,
                keys_arr,
                field_count,
            );
        if !stamped_from_cache {
            set_object_keys_with_live(obj_ptr, keys_arr, field_count);
            // #6804: birth-stamp the runtime ShapeId (see `ShapeCacheEntry`) —
            // newborn literals carry their stable identity immediately, so
            // typed_feedback tokens and the id-keyed FIELD_CACHE never see a
            // pre-stamp window for shape-cached objects.
            // #8113: `field_count` is the LOGICAL live-slot bound; the extra
            // physical slots above it stay available for dynamic growth.
            let rep = super::field_rep::REP_ANY;
            crate::object::shapes::birth_stamp_object_shape(
                obj_ptr,
                runtime_shape_id,
                field_count,
                rep,
            );
        }
    }

    obj_handle.get_raw_mut_ptr::<ObjectHeader>()
}

/// Clone a spread source object and reserve extra physical slot capacity for additional
/// static properties. Used to implement object spread: `{ ...src, key1: val1, key2: val2 }`.
///
/// - `src_f64`: the spread source object as a NaN-boxed f64 (POINTER_TAG or raw pointer)
/// - `extra_count`: number of additional static properties — reserves physical slot capacity
///   for them, but does NOT add their keys to the keys_array upfront. Codegen is expected to
///   call `js_object_set_field_by_name` for each static prop, which correctly overwrites keys
///   that already exist in the spread source (preserving JS "last key wins" semantics) and
///   appends new keys (using the reserved capacity).
/// - `_static_keys_ptr`/`_static_keys_len`: unused (kept for ABI compat). Previously these
///   were used to pre-populate static keys in keys_array, but that created duplicate entries
///   when a static key matched an existing spread key, and the linear-scan lookup returned
///   the first (stale) match instead of the intended last-key value.
///
/// Returns the new *mut ObjectHeader as an i64 raw pointer (NOT NaN-boxed).
/// The returned object's `field_count` equals the source's field_count (NOT src + extra),
/// but the physical allocation reserves enough slots so subsequent
/// `js_object_set_field_by_name` calls have somewhere to append.
#[no_mangle]
pub unsafe extern "C" fn js_object_clone_with_extra(
    src_f64: f64,
    extra_count: u32,
    _static_keys_ptr: *const u8,
    _static_keys_len: u32,
) -> *mut ObjectHeader {
    // Extract raw pointer from NaN-boxed f64
    let src_bits = src_f64.to_bits();
    let top16 = src_bits >> 48;
    let src_raw = if top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };

    let header_size = std::mem::size_of::<ObjectHeader>();

    // If source is invalid OR not a genuine heap object, create an empty object
    // with capacity for the static props. Physical slot count = max(extra_count,
    // 8) to match js_object_set_field_by_name's alloc_limit = max(field_count, 8).
    // The `top16 >= 0x7FF8` extraction above admits SSO/BigInt/INT32/negative-
    // double payloads and exotic headers (Map/Set/Promise/…) whose bytes are not
    // an ObjectHeader; deref'ing `field_count`/`keys_array` off them is type
    // confusion (#6070). The sole production caller (`js_structured_clone`)
    // already gates on GC_TYPE_OBJECT, so this only hardens against a future one.
    let src_is_object = src_raw >= 0x10000
        && !crate::value::addr_class::is_handle_band(src_raw)
        && matches!(
            crate::value::addr_class::try_read_gc_header(src_raw),
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT
        );
    if !src_is_object {
        let phys_slots = std::cmp::max(extra_count, crate::object::INLINE_SLOT_FLOOR as u32);
        let total_size = header_size + phys_slots as usize * 8;
        let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
        (*new_ptr).class_id = 0;
        (*new_ptr).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*new_ptr).meta = ptr::null_mut();
        let fields_ptr = (new_ptr as *mut u8).add(header_size) as *mut u64;
        for i in 0..phys_slots as usize {
            // GC_STORE_AUDIT(INIT): freshly allocated clone field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), crate::value::TAG_UNDEFINED);
        }
        crate::gc::layout_init_pointer_free(new_ptr as *mut u8);
        // Empty keys array with capacity reserved for the static props to come.
        let new_keys_arr = crate::array::js_array_alloc(extra_count);
        set_object_keys(new_ptr, crate::object::ObjectKeys::owned(new_keys_arr));
        return new_ptr;
    }

    let src_ptr = src_raw as *const ObjectHeader;
    // An accessor's value is its getter's result, and its slot holds the
    // accessor pair (`accessor_pair.rs`): such a source copies by [[Get]].
    if super::string_wrapper::length(src_raw).is_some()
        || super::key_attrs::object_summary(src_ptr) & super::key_attrs::SUMMARY_ACCESSOR != 0
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let src_h = scope.root_nanbox_f64(src_f64);
        let target = js_object_alloc(0, 0);
        let copied = js_object_assign_one(
            crate::value::js_nanbox_pointer(target as i64),
            src_h.get_nanbox_f64(),
        );
        return crate::value::js_nanbox_get_pointer(copied) as *mut ObjectHeader;
    }
    let src_field_count = crate::object::object_live_slot_count(src_ptr);

    // Physical slot capacity: src_field_count + extra_count, but at least max(fc, 8) to match
    // js_object_set_field's alloc_limit check. Extra slots are scratch space for subsequent
    // js_object_set_field_by_name calls.
    let phys_slots = std::cmp::max(
        src_field_count + extra_count,
        crate::object::INLINE_SLOT_FLOOR as u32,
    );
    let total_size = header_size + phys_slots as usize * 8;
    let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    (*new_ptr).class_id = 0;
    (*new_ptr).parent_class_id = 0;
    // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
    (*new_ptr).meta = ptr::null_mut();

    // Copy source fields (as raw f64/u64 words — preserves NaN-boxing)
    let src_fields = (src_ptr as *const u8).add(header_size) as *const u64;
    let dst_fields = (new_ptr as *mut u8).add(header_size) as *mut u64;
    for i in 0..src_field_count as usize {
        let field_val = *src_fields.add(i);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let cleaned = if field_val == 0x7FFD_0000_0000_0000 {
            eprintln!(
                "[CLONE_NULL_PTR] field {} from src={:p} — replacing with undefined",
                i, src_ptr
            );
            crate::value::TAG_UNDEFINED
        } else {
            field_val
        };
        // GC_STORE_AUDIT(INIT): cloned object is unpublished; layout is rebuilt after field copy.
        ptr::write(dst_fields.add(i), cleaned);
    }
    // Initialize scratch slots to undefined
    for i in src_field_count as usize..phys_slots as usize {
        // GC_STORE_AUDIT(INIT): cloned object scratch field slot is initialized pointer-free.
        ptr::write(dst_fields.add(i), crate::value::TAG_UNDEFINED);
    }
    rebuild_object_field_layout(new_ptr, src_field_count as usize);

    // #8113: publish the clone's live inline-slot bound BEFORE the first
    // allocation below. `gc_field_slot_range` reads the bound from the ShapeId
    // descriptor now, and everything from the arena allocation above to here is
    // allocation-free, so this closes the window in which the copied
    // pointer-bearing slots would be invisible to tracing (#7154/#7164).
    crate::object::shapes::birth_publish_object_shape(new_ptr, src_field_count);

    // Build keys array: copy ONLY src keys. Static keys are NOT added here — codegen uses
    // js_object_set_field_by_name for each static prop, which appends new keys via
    // js_array_push. Pre-size the keys capacity to avoid immediate reallocation on append.
    let src_keys = crate::object::object_keys(src_ptr);
    let new_keys_arr = crate::array::js_array_alloc(src_field_count + extra_count);
    let new_keys_elements =
        crate::array::array_elements_ptr(new_keys_arr as *const crate::array::ArrayHeader)
            as *mut f64;

    if !src_keys.is_null() && (src_keys.arr() as usize) >= 0x10000 {
        let (src_key_elements, src_key_len) = src_keys.dense_slots();
        let copy_count = src_key_len.min(src_field_count as usize);
        for i in 0..copy_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *new_keys_elements.add(i) = *src_key_elements.add(i);
        }
        (*new_keys_arr).length = copy_count as u32;
        rebuild_array_layout_from_slots(new_keys_arr);
    } else {
        (*new_keys_arr).length = 0;
    }

    // The clone's list is its own until it is published.
    set_object_keys(new_ptr, crate::object::ObjectKeys::owned(new_keys_arr));

    new_ptr
}

/// Copy all own enumerable fields from `src` into `dst`, using `js_object_set_field_by_name`
/// semantics (overwrite existing, append new). Used for multi-spread object literals like
/// `{...a, ...b}` to apply each additional spread after the first has been cloned via
/// `js_object_clone_with_extra`.
#[no_mangle]
pub unsafe extern "C" fn js_object_copy_own_fields(dst_i64: i64, src_f64: f64) {
    // Extract dst pointer (may be NaN-boxed or raw)
    let dst_bits = dst_i64 as u64;
    let dst_top16 = dst_bits >> 48;
    let dst_raw = if dst_top16 >= 0x7FF8 {
        (dst_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        dst_bits as usize
    };
    if dst_raw < 0x10000 {
        return;
    }
    let dst = dst_raw as *mut ObjectHeader;

    // Extract + VALIDATE the src pointer (2026-07-02 audit P0). The old
    // `top16 >= 0x7FF8` catch-all admitted SSO strings (0x7FF9), registry
    // handles, INT32s, and negative doubles, and the only guard was
    // `< 0x10000` — so `{...response}` (a POINTER-tagged fetch-band id) or
    // `{..."ab"}` deref'd a non-heap address as an ObjectHeader (Linux
    // SIGSEGV), and `{...map}` walked a MapHeader's bytes as object fields.
    // Spec (CopyDataProperties): non-objects with no own enumerable string
    // props contribute nothing — so anything that is not a genuine heap
    // OBJECT is skipped. (Known remaining gap, safe now instead of UB:
    // spreading a STRING should yield its index properties; it currently
    // yields none.)
    let src_bits = src_f64.to_bits();
    let src_top16 = src_bits >> 48;
    // Only a POINTER-tagged value can be a spreadable heap object.
    if src_top16 != 0x7FFD {
        return;
    }
    let src_raw = (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::value::addr_class::is_handle_band(src_raw) || src_raw < 0x10000 {
        return;
    }
    // Probe the GcHeader without deref-faulting and require a real object
    // (Maps/Sets/Promises/etc. have their own layouts — reading their bytes
    // as ObjectHeader fields is type confusion).
    match crate::value::addr_class::try_read_gc_header(src_raw) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return,
    }
    let src = src_raw as *const ObjectHeader;
    // A source with an accessor copies the GETTER's value (CopyDataProperties
    // is a [[Get]] per key). The raw walk below reads slots, and an accessor
    // key's slot holds its accessor pair (`accessor_pair.rs`), so such a
    // source takes the [[Get]]-based copy.
    if super::string_wrapper::length(src_raw).is_some()
        || super::key_attrs::object_summary(src) & super::key_attrs::SUMMARY_ACCESSOR != 0
    {
        js_object_assign_one(crate::value::js_nanbox_pointer(dst as i64), src_f64);
        return;
    }

    // #6667: a native-module namespace (`{ ...require("crypto") }`) stores no
    // real fields — only the internal `__module__` sentinel — so the raw
    // keys_array walk below would copy nothing usable. Enumerate + resolve its
    // export surface instead (the same list `Object.keys` returns), so wildcard
    // interop and object spread see the exports Node's namespace exposes.
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let dst_h = scope.root_raw_mut_ptr(dst);
        if super::native_module::copy_native_module_exports(src, |key_ptr, value| {
            js_object_set_field_by_name(dst_h.get_raw_mut_ptr::<ObjectHeader>(), key_ptr, value);
        }) {
            return;
        }
    }

    // Iterate src's keys and copy each value via set_field_by_name.
    let src_keys = crate::object::object_keys(src);
    if src_keys.is_null() || (src_keys.arr() as usize) < 0x10000 {
        return;
    }
    let key_count = src_keys.count() as usize;
    let src_field_count = crate::object::object_live_slot_count(src) as usize;
    let alloc_limit = std::cmp::max(src_field_count, crate::object::INLINE_SLOT_FLOOR);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let src_fields = (src as *const u8).add(header_size) as *const u64;

    // Iterate up to `key_count`, not `min(key_count, src_field_count)`.
    // For objects with overflow fields (≥9 keys) `src_field_count` caps
    // at the inline alloc_limit (8) and the values for slots ≥ 8 live
    // in OVERFLOW_FIELDS — without iterating to `key_count` and routing
    // slots ≥ alloc_limit through `js_object_get_field`, the copy
    // silently dropped 9th..Nth properties.
    // The shape answers for every key at once: an own key is hidden
    // only when the receiver is a class instance or its shape has a
    // private entry (#11791), and no key of this snapshot becomes one.
    let hide_private = crate::object::field_get_set::own_keys_may_hide(src);
    for i in 0..key_count {
        let key_val = src_keys.get(i as u32);
        // #1781: SSO-aware copy — pre-fix the `is_string()` here
        // silently dropped any ≤5-byte key stored as a SHORT_STRING_TAG
        // value, so `Object.assign(target, src)` lost `src.id`,
        // `src.tag`, `src.name`, etc. when those slots used inline SSO.
        // Route SSO through `js_get_string_pointer_unified` so the
        // destination set-by-name path sees a stable heap pointer.
        if !key_val.is_any_string() {
            continue;
        }
        // Private elements (`#x`) live in a class instance's keys_array but are
        // never copied by object spread / Object.assign.
        if hide_private && crate::object::instance_private_key_hidden(src, key_val) {
            continue;
        }
        let key_f64 = f64::from_bits(key_val.bits());
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        let field_f64 = if i < alloc_limit {
            let field_bits = *src_fields.add(i);
            f64::from_bits(field_bits)
        } else {
            let v = js_object_get_field(src, i as u32);
            f64::from_bits(v.bits())
        };
        js_object_set_field_by_name(dst, key_ptr, field_f64);
    }
}

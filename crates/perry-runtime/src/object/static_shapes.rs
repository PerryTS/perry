//! Design step 4: compiler-assigned ("static") ShapeIds, adopted at the
//! ordinary mint.
//!
//! The driver assigns every compiler-nameable birth shape an id in the static
//! band ([`shapes::STATIC_SHAPE_ID_END`]) and generated code embeds it as an
//! immediate. These entry points are the SEEDS: each runs the same mint every
//! shape takes ([`shapes::shape_descriptor_ensure_with_holes`]) with the static
//! id passed as `requested`, so after a seed the facts <-> id pairing lives in
//! the shape record and nowhere else. There is no id-to-shape table.
//!
//! * A class shape (`proto_id = CLASS | class_id`) is seeded by the per-class
//!   mint at class registration, which precedes every instance of the class.
//! * A literal shape (`proto_id = 0`) is seeded by generated code before any
//!   user module's init runs, from its key NAMES; the names go through the
//!   ordinary canonical keys trie, so the seeded record's keys ARE the node
//!   every later birth of that key list reaches. The driver links one seed
//!   function per program (the literal ids generated guards embed); its
//!   constructor hands it to [`js_shape_register_static_seed`], and each
//!   agent runs it through [`js_shape_run_static_seed`] — `main` right after
//!   `js_gc_init`, every other agent as it starts (`agent::enter_worker_agent`).
//!
//! Agents: a static id means the same facts in every agent, and each agent's
//! slab gets its own record under it when that agent runs the seed. The
//! worker serializer carries keys and values, never a ShapeId, so an object
//! replayed into another agent is minted there BY FACTS on first sight; the
//! tests in `static_shapes_tests.rs` pin both orders.

use super::shapes;
use crate::array::ArrayHeader;

/// Seed (or find) the birth shape of a class's instances under the static id
/// `requested`. `live` is the class's birth live bound (`>= key_count` for a
/// class born wide, else `key_count`). `requested == 0` means the driver
/// assigned no static id. Returns the id instances are stamped with:
/// `requested` whenever this is the first mint of these facts in this agent,
/// which class registration guarantees. `rep` is the class's birth rep
/// (charter step 5, T1), part of the facts the static id names.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_class_keys_static(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
    rep: u64,
) -> u32 {
    let id = shapes::publish_shape_result(shapes::class_birth_shape_ensure(
        keys as usize as *const ArrayHeader,
        key_count,
        live,
        class_id,
        rep,
        Some(requested).filter(|&id| id != 0),
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    note_static_request("class", requested, id);
    id
}

/// Seed a literal (plain, `proto_id = 0`) shape under the static id
/// `requested`, from `count` NUL-separated key names in `packed`, with the
/// birth live bound `live` (`>= count` for a literal born wide, else
/// `count`) and the birth rep `rep` (charter step 5, T1: the literal's `F64`
/// lanes, part of the facts the static id names). Returns the id a birth of
/// this key list now resolves to in this agent.
///
/// The seed IS the literal's own birth mint run early: the same
/// [`shapes::class_birth_shape_ensure`] its module init runs through
/// [`js_object_shape_id_for_class_keys_static`] (an anonymous literal class
/// has no vtable class, so class id 0 names the same plain prototype), with
/// the same rep. A seed and a lazy mint of equal (keys, live, rep) are
/// therefore one lookup by facts and one ShapeId.
#[no_mangle]
pub extern "C" fn js_shape_seed_plain(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
) -> u32 {
    if count == 0 || packed.is_null() || packed_len == 0 {
        return 0;
    }
    // SAFETY: compiler-owned static data of `packed_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let names: Vec<&[u8]> = crate::object::packed_key_names(bytes);
    if names.len() != count as usize {
        return 0;
    }
    let keys = unsafe { canonical_keys_for_names(&names) };
    let id = shapes::publish_shape_result(shapes::class_birth_shape_ensure(
        keys.arr(),
        keys.count(),
        live,
        0,
        rep,
        Some(requested),
    ));
    // SAFETY: as above.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    note_static_request("seed", requested, id);
    id
}

/// LLVM's `{ i32, ptr }` entry in the generated static seed unit. The
/// pointer names the defining body's one image-owned `JsFunctionInfo`, not a
/// closure object; fresh factory closures with the same body share it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConstFnStaticEntry {
    pub slot: u32,
    pub info: *const crate::closure::JsFunctionInfo,
}

/// Seed final shape facts before user code. This never allocates or stamps an
/// object; only the post-construction finalizer may publish these facts on a
/// receiver. All entries must name a permanent executable image.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn js_shape_seed_plain_constfn(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u32 {
    if requested == 0 || count == 0 || packed.is_null() || packed_len == 0 {
        invalid_constfn_static_seed();
    }
    // SAFETY: compiler-owned static data of `packed_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let names: Vec<&[u8]> = crate::object::packed_key_names(bytes);
    if names.len() != count as usize {
        invalid_constfn_static_seed();
    }
    let infos = checked_constfn_static_entries(entries, entry_count);
    let keys = unsafe { canonical_keys_for_names(&names) };
    let id = shapes::publish_shape_result(shapes::final_shape_ensure_constfn(
        keys.arr(),
        keys.count(),
        live,
        0,
        rep,
        &infos,
        Some(requested),
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    note_static_request("seed-constfn", requested, id);
    id
}

/// Mint final class/literal facts without stamping an object. Class
/// registration seeds these facts separately from its Any/F64 allocation
/// shape, after all prototype registrations; `requested == 0` requests a dynamic id.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn js_object_final_shape_id_for_class_keys_static_constfn(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u32 {
    let infos = checked_constfn_static_entries(entries, entry_count);
    let id = shapes::publish_shape_result(shapes::final_shape_ensure_constfn(
        keys as usize as *const ArrayHeader,
        key_count,
        live,
        class_id,
        rep,
        &infos,
        Some(requested).filter(|&id| id != 0),
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    note_static_request("class-constfn", requested, id);
    id
}

/// Promote a completed ordinary object, never an allocation, to a static
/// ConstFn shape. Refusals leave the receiver untouched. Return the refreshed
/// object handle because minting/setup may move it. Keys, body infos and the
/// requested id are compiler-owned permanent-image facts.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn js_object_finalize_constfn_static(
    object: u64,
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u64 {
    finalize_constfn_static(
        object,
        requested,
        packed,
        packed_len,
        count,
        live,
        class_id,
        rep,
        entries,
        entry_count,
        false,
    )
}

/// Thread reconstruction has completed all stores before this call. Unlike a
/// compiled allocation, its ordinary base rep may be Any; every F64 lane is
/// therefore validated from the current receiver before publishing final facts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finalize_constfn_static(
    object: u64,
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
    rebuilt: bool,
) -> u64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let root = scope.root_raw_mut_ptr(object as usize as *mut super::ObjectHeader);
    let Some(infos) = parse_constfn_static_entries(entries, entry_count) else {
        return object;
    };
    if packed.is_null() || packed_len == 0 || count == 0 || live < count {
        return object;
    }
    // SAFETY: compiler-owned bytes for this call, containing exact key names.
    let packed = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let Some(current) = root.with_mut_ptr::<super::ObjectHeader, _>(|obj| unsafe {
        finalized_constfn_facts(obj, packed, count, live, class_id, rep, &infos, rebuilt)
    }) else {
        // Validation only reads inline data and Rust-owned metadata; it cannot collect.
        return object;
    };
    let requested = Some(requested).filter(|&id| id != 0);
    // A record already under the requested id (the seed's, or a worker's
    // installed seed) is the only shape this receiver may take under that
    // id. The mint would either hit it by facts or abort on the miss, and a
    // miss is reachable from an ordinary receiver: equal key NAMES in a
    // different keys array. Compare the complete record here instead; a match
    // is stamped without minting, anything else is a refusal that leaves the
    // receiver untouched.
    if let Some((id, existing)) =
        requested.and_then(|id| shapes::shape_descriptor_by_id(id).map(|d| (id, d)))
    {
        return root.with_mut_ptr::<super::ObjectHeader, _>(|obj| {
            if final_record_names_receiver(&existing, &current, count, live, rep, &infos) {
                // SAFETY: `obj` is the rooted receiver validated above;
                // nothing between that validation and here can collect.
                unsafe { shapes::stamp_object_shape_id_with_carrier_note(obj, id) };
                note_static_request("finalized-constfn", id, id);
            }
            obj as usize as u64
        });
    }
    // The mint must be treated as a collection point: the GC call-effects
    // classifier cannot prove it non-collecting (the key-attribute and
    // keys-array resolvers reach a collector). The receiver is therefore
    // rooted across it and re-read below. The raw keys argument is the rooted
    // receiver's keys array; if a collection inside the mint moved it, the
    // minted record names the old address, and the identity check after the
    // mint refuses instead of stamping it. No record sits under `requested`
    // (checked above), so the adoption cannot be refused.
    let (minted, obj) = root.across_mut::<super::ObjectHeader, _>(|| {
        shapes::final_shape_ensure_constfn(
            current.keys as usize as *const ArrayHeader,
            count,
            live,
            class_id,
            rep,
            &infos,
            requested,
        )
    });
    // Reload and revalidate after the mint; no closure address spans it.
    if let Some(current) =
        unsafe { finalized_constfn_facts(obj, packed, count, live, class_id, rep, &infos, rebuilt) }
    {
        if let Ok(id) = minted {
            // Still check the complete record here, including learned
            // deprecation, before publication.
            if shapes::shape_descriptor_by_id(id).is_some_and(|d| {
                final_record_names_receiver(&d, &current, count, live, rep, &infos)
            }) {
                unsafe { shapes::stamp_object_shape_id_with_carrier_note(obj, id) };
                note_static_request("finalized-constfn", requested.unwrap_or(0), id);
            }
        }
    }
    obj as usize as u64
}

/// Whether the shape record `d` describes the validated receiver `current`
/// with the finalized ConstFn facts: the same keys array (identity, not just
/// equal names), bounds, prototype and kind, a birth record with no learned
/// deprecation, and exactly the requested bodies.
fn final_record_names_receiver(
    d: &shapes::ShapeDescriptor,
    current: &shapes::ShapeDescriptor,
    count: u32,
    live: u32,
    rep: u64,
    infos: &[shapes::ConstFnSlotInfo],
) -> bool {
    d.keys == current.keys
        && d.logical_key_count == count
        && d.live_inline_slot_count == live
        && d.proto_id == current.proto_id
        && d.object_kind == current.object_kind
        && d.semantic_generation == 0
        && d.hole_count == 0
        && d.summary == 0
        && d.rep == rep
        && d.deprecation_targets() == (0, 0)
        && d.special_constfn_mask == infos.iter().fold(0, |mask, i| mask | (1 << i.slot))
        && d.constfn_infos() == infos
}

/// No getter, proxy, or user code runs during this validation. All slots are
/// read from the current rooted receiver and must be ordinary inline data.
unsafe fn finalized_constfn_facts(
    obj: *mut super::ObjectHeader,
    packed: &[u8],
    count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    infos: &[shapes::ConstFnSlotInfo],
    rebuilt: bool,
) -> Option<shapes::ShapeDescriptor> {
    if obj.is_null()
        || !shapes::shape_word_is_writable(obj)
        || !(*obj).meta.is_null()
        // A receiver whose shape names a recorded prototype keeps it: a
        // static class shape names the class's.
        || shapes::object_prototype_word(obj) != 0
    {
        return None;
    }
    let current = shapes::object_shape_descriptor(obj)?;
    if !current.object_kind.is_ordinary_layout()
        || current.logical_key_count != count
        || current.live_inline_slot_count != live
        || current.summary != 0
        || current.hole_count != 0
        || current.semantic_generation != 0
        || current.proto_id != shapes::class_proto_id(class_id)
        || (!rebuilt
            && current.rep
                != infos.iter().fold(rep, |r, i| {
                    super::field_rep::with_slot_rep(r, i.slot as u32, super::field_rep::REP_ANY)
                }))
        || current.special_constfn_mask != 0
    {
        return None;
    }
    let names: Vec<&[u8]> = packed.strip_suffix(&[0])?.split(|&b| b == 0).collect();
    if names.len() != count as usize || names.iter().any(|n| n.is_empty()) {
        return None;
    }
    let (keys, len) = super::keys_array_dense_slots(current.keys as usize as *const ArrayHeader);
    if keys.is_null() || len < count as usize {
        return None;
    }
    for (slot, name) in names.iter().enumerate() {
        let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
        if crate::string::js_string_key_bytes(
            crate::JSValue::from_bits((*keys.add(slot)).to_bits()),
            &mut short,
        )? != *name
        {
            return None;
        }
    }
    let fields = (obj as *const u8).add(std::mem::size_of::<super::ObjectHeader>()) as *const u64;
    if rebuilt {
        if super::field_rep::has_deprecated(rep) {
            return None;
        }
        for slot in 0..count.min(super::field_rep::REP_SLOTS) {
            if super::field_rep::slot_rep(rep, slot) == super::field_rep::REP_F64 {
                let bits = *fields.add(slot as usize);
                if super::field_rep::f64_slot_bits(bits) != Some(bits) {
                    return None;
                }
            }
        }
    }
    for entry in infos {
        if entry.slot as u32 >= count
            || super::field_rep::slot_rep(rep, entry.slot as u32) != super::field_rep::REP_SPECIAL
        {
            return None;
        }
        let bits = *fields.add(entry.slot as usize);
        if super::field_rep_store::constfn_store_info(bits)? != entry.info {
            return None;
        }
        let closure =
            (bits & crate::value::POINTER_MASK) as usize as *const crate::closure::ClosureHeader;
        let info = &*(*closure).info;
        // Arity is checked at method-site prime time. Every closure-specific
        // direct-call exclusion is checked now before making the shape claim.
        if info.code.is_null()
            || !matches!(
                crate::closure::resolve_strategy(info).kind(),
                crate::closure::DispatchKind::Arity(_)
            )
            || super::class_registry::is_class_object_value(f64::from_bits(bits))
            || super::global_this::is_function_prototype_object_value(f64::from_bits(bits))
            || super::native_module::bound_native_callable_module_and_method(f64::from_bits(bits))
                .is_some()
            || info.code == super::global_this::global_this_builtin_noop_thunk as *const u8
            || info.code == super::global_this::global_this_array_thunk as *const u8
        {
            return None;
        }
    }
    Some(current)
}

fn checked_constfn_static_entries(
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> Vec<shapes::ConstFnSlotInfo> {
    parse_constfn_static_entries(entries, entry_count)
        .unwrap_or_else(|| invalid_constfn_static_seed())
}

fn parse_constfn_static_entries(
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> Option<Vec<shapes::ConstFnSlotInfo>> {
    if entries.is_null() || entry_count == 0 || entry_count > super::field_rep::REP_SLOTS {
        return None;
    }
    // SAFETY: compiler-owned static data of `entry_count` ABI entries.
    let entries = unsafe { std::slice::from_raw_parts(entries, entry_count as usize) };
    let mut infos = Vec::with_capacity(entries.len());
    let mut previous = None;
    for entry in entries {
        if entry.slot >= super::field_rep::REP_SLOTS
            || previous.is_some_and(|slot| entry.slot <= slot)
            || entry.info.is_null()
        {
            return None;
        }
        // SAFETY: codegen names a `JsFunctionInfo` in its permanent image.
        let info = unsafe { &*entry.info };
        if info.flags & crate::codegen_abi::FN_PERMANENT_IMAGE == 0 {
            return None;
        }
        infos.push(shapes::ConstFnSlotInfo {
            slot: entry.slot as u8,
            info: entry.info as usize as u64,
        });
        previous = Some(entry.slot);
    }
    Some(infos)
}

#[cold]
fn invalid_constfn_static_seed() -> ! {
    eprintln!("Perry internal error: invalid ConstFn static seed facts");
    std::process::abort();
}

/// The static hit census (`PERRY_STATIC_SHAPE_CENSUS=1`): one stderr line per
/// static-id request — which mint (`seed`, `class`, `typed`), the id
/// requested, the id the mint returned. `requested != got` is a static MISS:
/// the facts were already minted under another id, so births of this content
/// stamp `got` while the guards compare the immediate `requested`. Requests
/// happen at seeds, module init and completed ConstFn reconstruction. A
/// reconstruction requests 0 (lookup by facts), so it must still report the
/// actual published id; other unassigned requests remain outside the census.
pub(crate) fn note_static_request(path: &str, requested: u32, got: u32) {
    static CENSUS_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if (requested != 0 || path == "finalized-constfn")
        && *CENSUS_ENABLED.get_or_init(|| std::env::var_os("PERRY_STATIC_SHAPE_CENSUS").is_some())
    {
        let verdict = if requested == got { "hit" } else { "miss" };
        eprintln!("perry-static-shape: {path} requested={requested:#x} got={got:#x} {verdict}");
    }
}

/// The program's seed function, registered by the generated seed unit's
/// constructor. One function pointer for the process (every agent runs the
/// same seeds); `0` = the program has no literal shape to seed.
static STATIC_SEED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Register the program's seed function (called from a `global_ctors` entry
/// of the generated seed unit, before `main`; it only stores the pointer,
/// since no agent's heap exists yet).
#[no_mangle]
pub extern "C" fn js_shape_register_static_seed(seed: extern "C" fn()) {
    STATIC_SEED.store(seed as usize, std::sync::atomic::Ordering::Release);
}

/// Run the program's seed function in THIS agent, if one is registered:
/// `main` calls it right after `js_gc_init`, a worker before its module body,
/// so every static literal id is minted before any user code can mint those
/// facts under a counter id.
#[no_mangle]
pub extern "C" fn js_shape_run_static_seed() {
    let seed = STATIC_SEED.load(std::sync::atomic::Ordering::Acquire);
    if seed != 0 {
        // SAFETY: only `js_shape_register_static_seed` stores a non-zero
        // value, and it stores an `extern "C" fn()`.
        let seed: extern "C" fn() = unsafe { std::mem::transmute::<usize, extern "C" fn()>(seed) };
        seed();
    }
}

/// The canonical keys node for `names`, built exactly as a class keys array
/// is (`js_build_class_keys_array`): interned strings in a long-lived array,
/// then the canonical trie.
///
/// A seed runs before any module's pool; the builder mints each name's atom
/// first, so the canonical copy stores the atoms the pools find later
/// (`build_longlived_keys_array`).
///
/// # Safety
/// May collect; holds no caller object.
pub(crate) unsafe fn canonical_keys_for_names(names: &[&[u8]]) -> super::ObjectKeys {
    let _immortal = crate::gc::ImmortalLayoutScope::new();
    let arr = super::alloc::build_longlived_keys_array(std::ptr::null_mut(), 0, names);
    crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
    super::canonical_keys::canonicalize(
        &super::canonical_keys::SharedLayout::shape_cache_entry(),
        arr,
        names.len() as u32,
    )
    .view()
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_OBJECT_SHAPE_ID_FOR_CLASS_KEYS_STATIC: extern "C" fn(
    u64,
    u32,
    u32,
    u32,
    u32,
    u64,
) -> u32 = js_object_shape_id_for_class_keys_static;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_SEED_PLAIN: extern "C" fn(u32, *const u8, u32, u32, u32, u64) -> u32 =
    js_shape_seed_plain;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_SEED_PLAIN_CONSTFN: extern "C" fn(
    u32,
    *const u8,
    u32,
    u32,
    u32,
    u64,
    *const ConstFnStaticEntry,
    u32,
) -> u32 = js_shape_seed_plain_constfn;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_OBJECT_FINAL_SHAPE_ID_FOR_CLASS_KEYS_STATIC_CONSTFN: extern "C" fn(
    u64,
    u32,
    u32,
    u32,
    u32,
    u64,
    *const ConstFnStaticEntry,
    u32,
) -> u32 = js_object_final_shape_id_for_class_keys_static_constfn;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_REGISTER_STATIC_SEED: extern "C" fn(extern "C" fn()) =
    js_shape_register_static_seed;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_RUN_STATIC_SEED: extern "C" fn() = js_shape_run_static_seed;

#[cfg(test)]
#[path = "static_shapes_tests.rs"]
mod tests;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_OBJECT_FINALIZE_CONSTFN_STATIC: extern "C" fn(
    u64,
    u32,
    *const u8,
    u32,
    u32,
    u32,
    u32,
    u64,
    *const ConstFnStaticEntry,
    u32,
) -> u64 = js_object_finalize_constfn_static;

#[cfg(all(test, target_os = "linux"))]
#[path = "constfn_unload_tests.rs"]
mod unload_tests;

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

use super::shapes::{self, ShapeObjectKind};
use crate::array::ArrayHeader;

/// Seed (or find) the birth shape of a class's instances under the static id
/// `requested`. `live` is the class's birth live bound (`>= key_count` for a
/// class born wide, else `key_count`). `requested == 0` means the driver
/// assigned no static id. Returns the id instances are stamped with:
/// `requested` whenever this is the first mint of these facts in this agent,
/// which class registration guarantees.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_class_keys_static(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
) -> u32 {
    let id = shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_holes(
        keys as usize as *const ArrayHeader,
        key_count,
        live.max(key_count),
        0,
        ShapeObjectKind::Ordinary,
        0,
        shapes::class_proto_id(class_id),
        0,
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
/// `count`). Returns the id a birth of this key list now resolves to in this
/// agent.
#[no_mangle]
pub extern "C" fn js_shape_seed_plain(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
) -> u32 {
    if count == 0 || packed.is_null() || packed_len == 0 {
        return 0;
    }
    // SAFETY: compiler-owned static data of `packed_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let names: Vec<&[u8]> = bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
    if names.len() != count as usize {
        return 0;
    }
    let keys = unsafe { canonical_keys_for_names(&names) };
    let id = shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_holes(
        keys.arr(),
        keys.count(),
        live.max(keys.count()),
        0,
        ShapeObjectKind::Ordinary,
        0,
        shapes::PROTO_ID_DEFAULT,
        0,
        Some(requested),
    ));
    // SAFETY: as above.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    note_static_request("seed", requested, id);
    id
}

/// The static hit census (`PERRY_STATIC_SHAPE_CENSUS=1`): one stderr line per
/// static-id request — which mint (`seed`, `class`, `typed`), the id
/// requested, the id the mint returned. `requested != got` is a static MISS:
/// the facts were already minted under another id, so births of this content
/// stamp `got` while the guards compare the immediate `requested`. Requests
/// happen only at seeds and module init, so the gate costs one load there.
pub(crate) fn note_static_request(path: &str, requested: u32, got: u32) {
    static CENSUS_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if requested != 0
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
) -> u32 = js_object_shape_id_for_class_keys_static;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_SEED_PLAIN: extern "C" fn(u32, *const u8, u32, u32, u32) -> u32 =
    js_shape_seed_plain;

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

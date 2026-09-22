//! The class ids of runtime classes whose instances are ORDINARY objects
//! (#340/#341) — one place that says which id belongs to which family.
//!
//! Honest tags gives every native-backed value a `GC_TYPE_OBJECT` carrying a
//! family class id, and those ids were being minted family by family as bare
//! constants in the family's own module. There is no central allocator for
//! class ids in the tree, per-module class-id collisions are a known open
//! problem (#10824: a class id must also never alias a live `ShapeId`), and
//! the worker-transfer guard had grown into a range spelled
//! `TEXT_ENCODER_CLASS_ID..=IMMEDIATE_CLASS_ID` **inside `text.rs`** — a check
//! about every family, living in one family's file, widened by hand each time
//! a family landed.
//!
//! This module owns the `0xFFFF_24xx` web-builtin block instead. A family
//! declares its own alias from here (so its call sites are unchanged), the
//! range is closed by construction, and the const assertions below fail the
//! build rather than a test if two families ever name one id or if an id
//! wanders into the `ShapeId` window.
//!
//! **Allocating an id: take the next value, add it to `ALL` and to the
//! `is_native_backed_class_id` range if the family owns native state.** Do not
//! reuse a retired id: a stale compiled artifact naming it would brand the
//! wrong family.

/// Web-builtin block start. `ShapeId`s live in `[0x8000_0000, 0xC000_0000)`
/// (`object/shapes.rs`), so this block is far above them; the assertion at the
/// bottom of the file pins that rather than trusting the comment (#10824).
pub(crate) const WEB_BUILTIN_BLOCK_START: u32 = 0xFFFF_2401;

// --- Pre-existing, declared elsewhere and aliased here so the block is one
// --- list. These six are ordinary objects already, but their state is own
// --- fields rather than prototype accessors (#10823-shaped), so they are not
// --- yet `is_native_backed_class_id`.
pub(crate) const ABORT_CONTROLLER: u32 = 0xFFFF_2401;
pub(crate) const ABORT_SIGNAL: u32 = 0xFFFF_2402;
pub(crate) const EVENT: u32 = 0xFFFF_2403;
pub(crate) const CUSTOM_EVENT: u32 = 0xFFFF_2404;
pub(crate) const DOM_EXCEPTION: u32 = 0xFFFF_2405;
pub(crate) const EVENT_TARGET: u32 = 0xFFFF_2406;

// --- Migrated families. Each carries native state in `ObjectMeta.native_state`
// --- and is therefore refused by the worker-transfer guard.
pub(crate) const TEXT_ENCODER: u32 = 0xFFFF_2407;
pub(crate) const TEXT_DECODER: u32 = 0xFFFF_2408;
pub(crate) const TIMEOUT: u32 = 0xFFFF_2409;
pub(crate) const IMMEDIATE: u32 = 0xFFFF_240A;
pub(crate) const TUI_WIDGET: u32 = 0xFFFF_240B;
pub(crate) const TUI_STATE: u32 = 0xFFFF_240C;
pub(crate) const TUI_REF_BOX: u32 = 0xFFFF_240D;
pub(crate) const TUI_APP: u32 = 0xFFFF_240E;
pub(crate) const TUI_STDOUT: u32 = 0xFFFF_240F;
pub(crate) const TUI_FOCUS_MANAGER: u32 = 0xFFFF_2410;
pub(crate) const ASYNC_HOOK: u32 = 0xFFFF_2411;

/// #10926: `AsyncResource` is native-backed too, but keeps its LEGACY
/// `0xFFFF_0079`, which `instanceof` and `class_registry::parent_static`
/// already bake into emitted code -- moving a live class id is #10824's hazard
/// for no gain, so the range gets a legacy companion instead of a renumbering.
/// Outside the block, so it is not in `ALL`.
pub(crate) const ASYNC_RESOURCE_LEGACY: u32 = 0xFFFF_0079;

/// The first id in the native-state range and the last one, inclusive. Every
/// family between them carries a `native_state` word the far side of a
/// `postMessage` could not reconstruct.
const NATIVE_BACKED_FIRST: u32 = TEXT_ENCODER;
const NATIVE_BACKED_LAST: u32 = ASYNC_HOOK;

/// Class ids whose instances are ordinary objects carrying native state that
/// cannot cross a thread boundary (#340/#341).
///
/// A contiguous range, so a family joins it by taking the next id rather than
/// by editing the transfer path. `thread.rs` calls this in its
/// `GC_TYPE_OBJECT` arm: an ordinary object no longer hits the "kinds 13-16
/// are native handles" rejection, so without this a `postMessage`d handle
/// would arrive as a plain `{}` with no native state instead of the named
/// `TypeError` #6185 made these surface.
pub(crate) fn is_native_backed_class_id(class_id: u32) -> bool {
    (NATIVE_BACKED_FIRST..=NATIVE_BACKED_LAST).contains(&class_id)
        || class_id == ASYNC_RESOURCE_LEGACY
}

/// Every id this module hands out, newest last. Used by the assertions below
/// and by the uniqueness test.
const ALL: &[u32] = &[
    ABORT_CONTROLLER,
    ABORT_SIGNAL,
    EVENT,
    CUSTOM_EVENT,
    DOM_EXCEPTION,
    EVENT_TARGET,
    TEXT_ENCODER,
    TEXT_DECODER,
    TIMEOUT,
    IMMEDIATE,
    TUI_WIDGET,
    TUI_STATE,
    TUI_REF_BOX,
    TUI_APP,
    TUI_STDOUT,
    TUI_FOCUS_MANAGER,
    ASYNC_HOOK,
];

/// Strictly ascending ⟹ no two families share an id, and the block stays
/// dense so `is_native_backed_class_id` can remain one range compare. A
/// `const fn` loop rather than a test: a duplicated id must not be able to
/// reach a build at all, and a `debug_assert` would enforce nothing in release
/// (#10824 was a SHIPPED aliasing bug).
const fn strictly_ascending(ids: &[u32]) -> bool {
    let mut i = 1;
    while i < ids.len() {
        if ids[i - 1] >= ids[i] {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(strictly_ascending(ALL), "two families claim one class id");
const _: () = assert!(ALL[0] == WEB_BUILTIN_BLOCK_START);
// A class id must never alias a live ShapeId: the shape store mints into
// `[0x8000_0000, 0xC000_0000)` and a collision would make a shape compare
// answer for a family brand.
const _: () = assert!(NATIVE_BACKED_FIRST >= 0xC000_0000);
const _: () = assert!(NATIVE_BACKED_LAST == ALL[ALL.len() - 1]);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every family that owns a `native_state` word is inside the transfer
    /// guard's range, and the classes that do NOT own one are outside it. The
    /// second half is what a new family gets wrong: taking the next id makes
    /// it non-transferable automatically, which is right, but an id added
    /// BELOW the range would silently ship a family that deep-copies into a
    /// worker as an empty object.
    #[test]
    fn the_transfer_guard_covers_exactly_the_migrated_families() {
        for id in [
            TEXT_ENCODER,
            TEXT_DECODER,
            TIMEOUT,
            IMMEDIATE,
            TUI_WIDGET,
            TUI_STATE,
            TUI_REF_BOX,
            TUI_APP,
            TUI_STDOUT,
            TUI_FOCUS_MANAGER,
            ASYNC_HOOK,
            ASYNC_RESOURCE_LEGACY,
        ] {
            assert!(
                is_native_backed_class_id(id),
                "{id:#x} carries native state but crosses a thread boundary"
            );
        }
        for id in [
            ABORT_CONTROLLER,
            ABORT_SIGNAL,
            EVENT,
            CUSTOM_EVENT,
            DOM_EXCEPTION,
            EVENT_TARGET,
            0,
            1,
            0x8000_0000,
        ] {
            assert!(!is_native_backed_class_id(id), "{id:#x} is not migrated");
        }
    }
}

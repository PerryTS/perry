//! Reads through an agent's ordinary shape directory: its address, its
//! property-atom index, and the positional key front of a record.
use super::{ShapeSlab, PROTO_ID_PER_OBJECT};
use crate::array::ArrayHeader;
use crate::codegen_abi::LayoutWord;

/// The address of this thread's ordinary shape-directory mirror, which
/// [`positional_key_words`] reads through (`agent_ptrs` slot
/// `AGENT_PTR_SHAPE_DIR`).
#[inline]
pub(crate) fn ordinary_dir_addr() -> *const u8 {
    ShapeSlab::ordinary_dir_addr()
}

/// The property-atom index (`string::intern`) the directory at `dir` names:
/// null until the agent's first atom, and for `PERRY_EMPTY_SHAPE_DIR`. One
/// load from the directory in hand; no thread-local is named, so the
/// directory's static stays private to its module.
///
/// # Safety
/// `dir` is an agent's ordinary directory or `PERRY_EMPTY_SHAPE_DIR`.
#[inline(always)]
pub(crate) unsafe fn dir_atoms(dir: *const u8) -> *const u8 {
    *(dir.add(super::shapes_store::DIR_ATOMS_OFFSET) as *const *const u8)
}

/// `string::intern` publishes the agent's atom index when its slots move.
pub(crate) fn publish_agent_atoms(table: *const u8) {
    ShapeSlab::publish_agent_atoms(table);
}

/// `string::intern` retracts the agent's atom index when it is dropped.
pub(crate) fn retract_agent_atoms(table: *const u8) {
    ShapeSlab::retract_agent_atoms(table);
}

/// [`positional_key_words`] for the computed-key read
/// (`object::dynamic_key_read`), which also refuses a shape whose
/// [[Prototype]] identity is PER-OBJECT.
///
/// A read site's front is reached only past its own receiver tests; the
/// computed-key read has no site, so it takes this exclusion from the shape
/// itself. A per-object identity is how an exotic read receiver
/// (`process.env`, an arguments object) and a module namespace project "its
/// reads are not answered by its key list" into their shape
/// ([`object_proto_id`]), so such a list naming the key proves nothing.
///
/// # Safety
/// As [`positional_key_words`].
#[inline]
pub(crate) unsafe fn plain_positional_key_words(
    dir: *const u8,
    shape_id: LayoutWord,
) -> Option<(PositionalKeys, usize)> {
    // A shadowed receiver can have deleted or overridden inline properties;
    // its record alone does not prove a present own data slot.
    if shape_id.0 & super::super::overflow::SHADOW_BIT != 0 {
        return None;
    }
    let r = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    if r.proto_id == PROTO_ID_PER_OBJECT {
        return None;
    }
    super::super::key_attrs::debug_assert_positional(
        r.keys as usize as *const ArrayHeader,
        r.position_bound_raw(),
    );
    Some((
        PositionalKeys(r.keys as usize as *const ArrayHeader),
        r.position_bound_raw() as usize,
    ))
}

/// A record's canonical keys array, for [`positional_key_words`]: its words
/// are asked only once POSBOUND is known to be nonzero, so the front-offset
/// arithmetic runs only on the path that reads a key.
pub(crate) struct PositionalKeys(*const ArrayHeader);

impl PositionalKeys {
    /// The first logical key word.
    ///
    /// # Safety
    /// Only when the record's POSBOUND is nonzero: then `keys` names a live
    /// keys array (the collector marks through and rewrites `keys`) holding
    /// at least POSBOUND logical keys. Logical element `i` sits past the
    /// array's FRONT OFFSET, which a canonical list can carry without ever
    /// being shifted (a size-class round-up alone makes the physical capacity
    /// exceed the logical one: `keys_front_offset_tests`), so the accessor is
    /// asked, never `+8`.
    #[inline(always)]
    pub(crate) unsafe fn words(&self) -> *const u64 {
        crate::array::array_elements_ptr(self.0) as *const u64
    }
}

/// The keys of shape `shape_id` that are positional, and the position bound.
///
/// # Safety
/// `dir` is this agent's ordinary directory.
#[cfg(test)]
pub(crate) unsafe fn positional_key_words(
    dir: *const u8,
    shape_id: LayoutWord,
) -> Option<(PositionalKeys, usize)> {
    let r = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    super::super::key_attrs::debug_assert_positional(
        r.keys as usize as *const ArrayHeader,
        r.position_bound_raw(),
    );
    Some((
        PositionalKeys(r.keys as usize as *const ArrayHeader),
        r.position_bound_raw() as usize,
    ))
}

/// Does shape `shape_id` store the key whose NaN-boxed bits are `key_bits`
/// at inline slot `guess`, by position? See [`positional_key_words`].
///
/// # Safety
/// As [`positional_key_words`].
#[cfg(test)]
pub(crate) unsafe fn slot_guess_confirmed(
    dir: *const u8,
    shape_id: LayoutWord,
    key_bits: u64,
    guess: usize,
) -> bool {
    positional_key_words(dir, shape_id)
        .is_some_and(|(keys, bound)| guess < bound && *keys.words().add(guess) == key_bits)
}

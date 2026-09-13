//! Search positions carried across JavaScript calls on non-ASCII strings
//! (#10164).
//!
//! A JavaScript `exec`, `test`, `search` or `matchAll` step runs one search per
//! call and binds its subject afresh. On a non-ASCII (WTF-8) string a search
//! that starts at `lastIndex` without a position pays a seek from the nearer end
//! of the string, so a loop over one long string does quadratic work. Within one
//! compound operation (`split`, `replace`, global `match`) `perex_api::Reuse`
//! already carries the position; this carries it from one call to the next.
//!
//! The next call must be searching the same string, and that is decided without
//! a traced edge or any per-object state: the string's address, its byte and
//! UTF-16 lengths and the thread's heap generation must all be unchanged.
//! `gc::heap_generation` advances on every event that frees or moves heap
//! memory, so an unchanged generation means the object at that address was
//! neither freed (letting another string take the address) nor moved. The
//! lengths reject an in-place append, the only way a live string's bytes change.
//! A wrong position could only give wrong answers, never unsafety (the contract
//! of `perex::input::Position`), and Perex still refuses one whose layout does
//! not match.
//!
//! The table is per thread, four entries of plain data. RegExp objects gain no
//! state (their header stays one 56-byte record) and the collector has nothing
//! new to scan: the address is kept only as a concealed identity, never read
//! back as a pointer.

use crate::gc::RuntimeHandle;
use crate::string::StringHeader;
use perex::input::Position;
use std::cell::Cell;

/// Which string a position belongs to, as observed at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StringIdentity {
    concealed_address: usize,
    generation: u64,
    byte_len: u32,
    utf16_len: u32,
}

#[derive(Clone, Copy)]
struct Hint {
    identity: StringIdentity,
    position: Position,
}

const HINTS: usize = 4;

crate::perry_thread_local! {
    static HINT_TABLE: Cell<[Option<Hint>; HINTS]> = const { Cell::new([None; HINTS]) };
    static HINT_NEXT: Cell<u8> = const { Cell::new(0) };
}

#[cfg(test)]
crate::perry_thread_local! {
    static HINT_USES: Cell<u64> = const { Cell::new(0) };
    static HINTS_DISABLED: Cell<bool> = const { Cell::new(false) };
}

/// Not a pointer to anything: a bijection of the address, compared for equality
/// only.
#[inline]
fn conceal(address: usize) -> usize {
    address.rotate_left(29) ^ 0x5a5a_5a5a_5a5a_5a5a_u64 as usize
}

/// The identity of the string `input` currently holds, when it is non-ASCII.
/// ASCII strings seek in constant work and need no position.
#[inline]
pub(crate) fn identity_of(input: &RuntimeHandle<'_>) -> Option<StringIdentity> {
    input.with_const_ptr::<StringHeader, _>(|s| unsafe { identity_of_header(s) })
}

/// [`identity_of`] for a header the caller is already reading.
///
/// # Safety
/// `s` must point at a live `StringHeader`.
#[inline]
pub(crate) unsafe fn identity_of_header(s: *const StringHeader) -> Option<StringIdentity> {
    let (byte_len, utf16_len) = ((*s).byte_len, (*s).utf16_len);
    (byte_len != utf16_len).then(|| StringIdentity {
        concealed_address: conceal(s as usize),
        generation: crate::gc::heap_generation::heap_generation(),
        byte_len,
        utf16_len,
    })
}

/// The position the last search on this same string stopped at, if any.
#[inline]
pub(crate) fn lookup(identity: StringIdentity) -> Option<Position> {
    #[cfg(test)]
    if HINTS_DISABLED.with(Cell::get) {
        return None;
    }
    let found = HINT_TABLE.with(|table| {
        table
            .get()
            .iter()
            .flatten()
            .find(|hint| hint.identity == identity)
            .map(|hint| hint.position)
    });
    #[cfg(test)]
    if found.is_some() {
        HINT_USES.with(|n| n.set(n.get() + 1));
    }
    found
}

/// Remember where a search on this string stopped.
#[inline]
pub(crate) fn record(identity: StringIdentity, position: Position) {
    HINT_TABLE.with(|table| {
        let mut hints = table.get();
        let slot = match hints
            .iter()
            .position(|hint| hint.is_some_and(|hint| hint.identity == identity))
        {
            Some(slot) => slot,
            None => HINT_NEXT.with(|next| {
                let slot = next.get() as usize % HINTS;
                next.set(((slot + 1) % HINTS) as u8);
                slot
            }),
        };
        hints[slot] = Some(Hint { identity, position });
        table.set(hints);
    });
}

#[cfg(test)]
pub(crate) fn hint_uses() -> u64 {
    HINT_USES.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn clear_for_test() {
    HINT_TABLE.with(|table| table.set([None; HINTS]));
    HINT_USES.with(|n| n.set(0));
}

/// Searches on this thread ignore recorded positions while this is held, so a
/// test can measure the unpositioned cost next to the positioned one.
#[cfg(test)]
pub(crate) struct DisableHintsForTest(bool);

#[cfg(test)]
impl DisableHintsForTest {
    pub(crate) fn new() -> Self {
        Self(HINTS_DISABLED.with(|d| d.replace(true)))
    }
}

#[cfg(test)]
impl Drop for DisableHintsForTest {
    fn drop(&mut self) {
        HINTS_DISABLED.with(|d| d.set(self.0));
    }
}

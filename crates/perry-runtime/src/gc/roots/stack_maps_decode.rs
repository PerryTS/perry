//! The eager, whole-section GC-map parser.
//!
//! It is no longer the parser the collector uses — [`super::lazy`] answers
//! frame lookups without materialising anything — but it is still SHIPPED, for
//! three reasons that are all the same reason:
//!
//! * `PERRY_GC_STACK_MAP_EAGER=1` restores it as the index, so one binary can
//!   be A/B'd between the two;
//! * `PERRY_GC_STACK_MAP_CROSSCHECK=1` builds it alongside the lazy index and
//!   asserts, per frame, that the two answer identically;
//! * it is the reference the lazy path is checked against, which only means
//!   anything if it is the same code the reference describes.
//!
//! And it does not contain a second decoder. It drives the same
//! [`RecordWalk`](super::lazy::RecordWalk) the lazy lookup drives, over whole
//! blobs instead of single functions. That is the load-bearing part of the
//! safety argument: the two paths cannot disagree about what a record says,
//! because only one piece of code reads one.

use super::lazy::{collect_derived, collect_slots, Payload, RecordWalk, Step};
use super::{StackMapDerived, StackMapLocation, StackMapRecord, GC_MAP_VERSION};

/// Decode every function entry in the section.
///
/// The linker concatenates the entries of every function it kept (v9: one
/// entry per function, each in an input section tied to its function's), so
/// the section is an array of fixed-size entries in link order.
///
/// `origin` is the runtime address of `bytes[0]` (see [`function_address`]).
pub(super) fn parse_gc_map(
    bytes: &[u8],
    origin: usize,
) -> Option<(
    Vec<StackMapRecord>,
    Vec<StackMapLocation>,
    Vec<StackMapDerived>,
)> {
    let mut records = Vec::new();
    let mut roots: Vec<StackMapLocation> = Vec::new();
    let mut derived: Vec<StackMapDerived> = Vec::new();

    for_each_entry(bytes, origin, |entry| {
        let blob = entry.records;
        let blob_end = blob.len();
        let function_address = entry.address;
        let stack_size = u64::from(entry.stack_size);
        let record_count = entry.record_count;
        // The repeat chain is per FUNCTION and so is the blob: the walk starts
        // after the function's instruction offsets with nothing to repeat.
        let mut walk = RecordWalk::new(blob, blob_end, entry.stream_at(), record_count);

        let mut previous: Option<(Payload, (u32, u32, u32, u32))> = None;
        for record in 0..record_count as usize {
            let instruction_offset = read_u32(blob, record.checked_mul(4)?)?;

            let payload = match walk.next() {
                Step::Record(payload) => payload,
                Step::Done | Step::Malformed => return None,
            };

            // 77% of records repeat the previous live set. Point them at
            // one copy instead of duplicating 154k entries — the same
            // sharing v4 did, keyed on the payload the walk reports.
            let range = match previous {
                Some((seen, range)) if seen == payload => range,
                _ => {
                    let start = u32::try_from(roots.len()).ok()?;
                    collect_slots(
                        blob,
                        payload.roots_at,
                        blob_end,
                        payload.roots_len,
                        &mut roots,
                    )?;
                    let derived_start = u32::try_from(derived.len()).ok()?;
                    collect_derived(blob, &payload, blob_end, &mut derived)?;
                    (start, payload.roots_len, derived_start, payload.derived_len)
                }
            };
            previous = Some((payload, range));

            records.push(StackMapRecord {
                pc: function_address.checked_add(instruction_offset as usize)?,
                function_address,
                stack_size,
                roots_start: range.0,
                roots_len: range.1,
                derived_start: range.2,
                derived_len: range.3,
            });
        }
        // The blob is exactly this function's offsets and stream: a stream
        // that ends early or runs on describes some other layout. This is what
        // lets the lazy lookup trust a blob it never walked to the end.
        if walk.cursor() != blob_end {
            return None;
        }
        Some(())
    })?;

    Some((records, roots, derived))
}

/// v9 entry: `i32 function - e, u32 stack_size, u32 record_count | version
/// << 24, i32 records - e, u32 records_len`, `e` being the entry's address.
pub(super) const FUNCTION_ENTRY_BYTES: usize = 20;
/// The record count's bits; the top byte is the format version.
const RECORD_COUNT_MASK: u32 = (1 << 24) - 1;

/// One validated v9 entry.
pub(super) struct Entry<'a> {
    pub(super) address: usize,
    pub(super) stack_size: u32,
    pub(super) record_count: u32,
    /// This function's records: `record_count` u32 instruction offsets, then
    /// its varint stream.
    pub(super) records: &'a [u8],
}

impl Entry<'_> {
    /// Offset of the varint stream within [`Self::records`].
    pub(super) fn stream_at(&self) -> usize {
        self.record_count as usize * 4
    }
}

/// Number of entry slots in `bytes`, for sizing the index exactly.
pub(super) fn entry_count(bytes: &[u8]) -> usize {
    bytes.len() / FUNCTION_ENTRY_BYTES
}

/// The one walk over a map section's entries. Both the lazy build and the
/// eager parser go through it, so the two cannot disagree about where an
/// entry is or whether it is valid.
///
/// Every entry is validated before `visit` sees it: the version, the record
/// count against the records blob, and the blob against the section (it may
/// sit before or after the entries, as the compiler's own section does, but
/// never on them). An all-zero slot is linker padding; a trailing partial
/// slot must be zero too.
pub(super) fn for_each_entry<'a>(
    bytes: &'a [u8],
    origin: usize,
    mut visit: impl FnMut(&Entry<'a>) -> Option<()>,
) -> Option<()> {
    let full = bytes.len() - bytes.len() % FUNCTION_ENTRY_BYTES;
    if bytes[full..].iter().any(|byte| *byte != 0) {
        return None;
    }
    for base in (0..full).step_by(FUNCTION_ENTRY_BYTES) {
        let slot = &bytes[base..base + FUNCTION_ENTRY_BYTES];
        if slot.iter().all(|byte| *byte == 0) {
            continue;
        }
        let counted = read_u32(bytes, base + 8)?;
        if counted >> 24 != u32::from(GC_MAP_VERSION) {
            return None;
        }
        let record_count = counted & RECORD_COUNT_MASK;
        let records_offset = read_u32(bytes, base + 12)? as i32 as isize;
        let records_len = read_u32(bytes, base + 16)? as usize;
        if (record_count as usize).checked_mul(4)? > records_len {
            return None;
        }
        let records_start = (base as isize).checked_add(records_offset)?;
        let records_end = records_start.checked_add(isize::try_from(records_len).ok()?)?;
        if records_start < full as isize && records_end > 0 {
            return None;
        }
        // SAFETY: the offset is a link-time difference between two labels of
        // the same object (`records - entry`), resolved by the linker exactly
        // like the function field this map already trusts, so it names
        // `records_len` bytes of the same loaded image — or, in a test, of the
        // same allocation. Image sections are never unmapped while their code
        // can be on a stack.
        let records: &'a [u8] = unsafe {
            core::slice::from_raw_parts(bytes.as_ptr().offset(records_start), records_len)
        };
        visit(&Entry {
            address: function_address(origin, base, read_u32(bytes, base)?)?,
            stack_size: read_u32(bytes, base + 4)?,
            record_count,
            records,
        })?;
    }
    Some(())
}

/// v6 (#11508): a function field is a signed 32-bit offset from its own entry,
/// resolved by the linker (`.long fn-entry`) so the section needs no
/// load-time relocations. `origin` is the runtime address of the section
/// slice's first byte and `entry` the entry's offset within it.
pub(super) fn function_address(origin: usize, entry: usize, field: u32) -> Option<usize> {
    origin
        .checked_add(entry)?
        .checked_add_signed(field as i32 as isize)
}

/// Used by the ELF section headers.
pub(super) fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

pub(super) fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// Only the ELF section lookup reads 64-bit fields since v6 dropped absolute
/// function addresses.
#[cfg(target_os = "linux")]
pub(super) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

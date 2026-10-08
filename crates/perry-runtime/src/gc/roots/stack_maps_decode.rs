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
use super::{StackMapDerived, StackMapLocation, StackMapRecord, GC_MAP_MAGIC, GC_MAP_VERSION};

/// Decode every concatenated compact map in the section.
///
/// The linker concatenates one directory per object file, so this walks them
/// one by one (see [`for_each_directory`]) rather than assuming a single map —
/// a decoder that reads only the first header silently drops every other
/// object's roots, which is invisible until a collection frees a live object.
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

    for_each_directory(bytes, |directory| {
        let blob = directory.records;
        let blob_end = blob.len();
        let stream_base = directory.stream_base();
        let mut walk = RecordWalk::new(blob, blob_end, stream_base, 0);

        for index in 0..directory.function_count {
            let function = directory.function(bytes, origin, index)?;
            let function_address = function.address;
            let stack_size = u64::from(function.stack_size);
            let record_count = function.record_count;

            // The recorded per-function offset must BE where the sequential
            // walk stands. The compiler proves this for every function of
            // every binary it emits (`verify_roundtrip`); proving it again
            // here, against the shipped bytes, is what lets the lazy lookup
            // start a walk at a recorded offset and trust the result.
            if stream_base.checked_add(function.stream_offset)? != walk.cursor() {
                return None;
            }

            // The repeat chain is per FUNCTION: the first record of a function
            // can never be a repeat, because the encoder re-arms there too.
            walk.restart(record_count);

            let mut previous: Option<(Payload, (u32, u32, u32, u32))> = None;
            for record in 0..record_count as usize {
                let instruction_offset = read_u32(
                    blob,
                    function.first_record.checked_add(record)?.checked_mul(4)?,
                )?;

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
        }
        Some(())
    })?;

    Some((records, roots, derived))
}

/// v8 directory header: magic, version, reserved byte, flags, function count,
/// total length (stride to the next directory), records offset, records
/// length, record total, reserved word.
pub(super) const DIRECTORY_HEADER_BYTES: usize = 32;

/// v8 directory entry: `i32 function_offset, u32 stack_size,
/// u32 record_count, u32 first_record, u32 stream_offset`.
pub(super) const FUNCTION_ENTRY_BYTES: usize = 20;

/// One validated v8 directory.
pub(super) struct Directory<'a> {
    /// Offset of the directory's first byte within its section.
    pub(super) base: usize,
    pub(super) function_count: usize,
    /// The records blob the header names: the `u32` instruction offsets of
    /// every record, then the varint stream.
    pub(super) records: &'a [u8],
    pub(super) record_total: usize,
}

/// One function of a [`Directory`], with its fields checked against the
/// directory they came from.
pub(super) struct DirectoryFunction {
    pub(super) address: usize,
    pub(super) stack_size: u32,
    pub(super) record_count: u32,
    /// Index of the function's first record in the instruction-offset array.
    pub(super) first_record: usize,
    /// Start of the function's records, relative to the stream's start.
    pub(super) stream_offset: usize,
}

impl Directory<'_> {
    /// Offset of the varint stream within [`Self::records`].
    pub(super) fn stream_base(&self) -> usize {
        self.record_total * 4
    }

    /// Function `index`. Its bounds were proven by [`for_each_directory`],
    /// which read every entry before handing the directory out.
    pub(super) fn function(
        &self,
        section: &[u8],
        origin: usize,
        index: usize,
    ) -> Option<DirectoryFunction> {
        let at = self
            .base
            .checked_add(DIRECTORY_HEADER_BYTES)?
            .checked_add(index.checked_mul(FUNCTION_ENTRY_BYTES)?)?;
        Some(DirectoryFunction {
            address: function_address(origin, self.base, read_u32(section, at)?)?,
            stack_size: read_u32(section, at + 4)?,
            record_count: read_u32(section, at + 8)?,
            first_record: read_u32(section, at + 12)? as usize,
            stream_offset: read_u32(section, at + 16)? as usize,
        })
    }
}

/// Sum of the function counts of every directory in `bytes`, reading headers
/// only. `None` for a section that does not walk; the full parse will refuse
/// it with the reason.
pub(super) fn directory_function_count(bytes: &[u8]) -> Option<usize> {
    let mut count = 0usize;
    walk_headers(bytes, |base, function_count, _total| {
        let _ = base;
        count = count.checked_add(function_count)?;
        Some(())
    })?;
    Some(count)
}

/// The one walk over a map section's directories. Both the lazy build and
/// the eager parser go through it, so the two cannot disagree about where a
/// directory starts, how long it is, or whether it is valid.
///
/// Every directory is validated before `visit` sees it: the version, the
/// flags, the length against the entries, the records blob against the
/// directory (it may follow the directory in-line, as the decode tests lay it
/// out, or live in a section of its own, as the compiler emits it — but it may
/// never overlap the entries), and every entry's record range and stream
/// offset against the records blob.
pub(super) fn for_each_directory<'a>(
    bytes: &'a [u8],
    mut visit: impl FnMut(&Directory<'a>) -> Option<()>,
) -> Option<()> {
    walk_headers(bytes, |base, function_count, total_len| {
        let entries_end = base
            .checked_add(DIRECTORY_HEADER_BYTES)?
            .checked_add(function_count.checked_mul(FUNCTION_ENTRY_BYTES)?)?;
        if entries_end > base.checked_add(total_len)? {
            return None;
        }
        let records_offset = read_u32(bytes, base + 16)? as i32 as isize;
        let records_len = read_u32(bytes, base + 20)? as usize;
        let record_total = read_u32(bytes, base + 24)? as usize;
        if read_u32(bytes, base + 28)? != 0 {
            return None;
        }
        if record_total.checked_mul(4)? > records_len {
            return None;
        }
        // The records may sit before or after the directory, never on it.
        let records_start = (base as isize).checked_add(records_offset)?;
        let entries_start = base as isize;
        let records_end = records_start.checked_add(isize::try_from(records_len).ok()?)?;
        if records_start < entries_end as isize && records_end > entries_start {
            return None;
        }
        // SAFETY: the offset is a link-time difference between two labels of
        // the same object (`_perry_gc_rec - _perry_gc_map`), resolved by the
        // linker exactly like the function fields this map already trusts, so
        // it names `records_len` bytes of the same loaded image — or, in a
        // test, of the same allocation. Image sections are never unmapped
        // while their code can be on a stack.
        let records: &'a [u8] = unsafe {
            core::slice::from_raw_parts(bytes.as_ptr().offset(records_start), records_len)
        };
        let directory = Directory {
            base,
            function_count,
            records,
            record_total,
        };
        // Prove every entry against the records before anyone walks them: the
        // encoder emits functions in stream order, so the first record index
        // is a running sum, the first stream offset is 0 and the rest never
        // decrease.
        let stream_len = records_len - record_total * 4;
        let mut next_record = 0usize;
        let mut previous_stream_offset = 0usize;
        for index in 0..function_count {
            let at = base + DIRECTORY_HEADER_BYTES + index * FUNCTION_ENTRY_BYTES;
            let record_count = read_u32(bytes, at + 8)? as usize;
            let first_record = read_u32(bytes, at + 12)? as usize;
            let stream_offset = read_u32(bytes, at + 16)? as usize;
            if first_record != next_record {
                return None;
            }
            next_record = next_record.checked_add(record_count)?;
            if next_record > record_total {
                return None;
            }
            if (index == 0 && stream_offset != 0)
                || stream_offset < previous_stream_offset
                || stream_offset > stream_len
            {
                return None;
            }
            previous_stream_offset = stream_offset;
        }
        if next_record != record_total {
            return None;
        }
        visit(&directory)
    })
}

/// Walk the directory headers of a section: magic, version, flags, and a
/// length that advances the cursor. `visit(base, function_count, total_len)`.
fn walk_headers(
    bytes: &[u8],
    mut visit: impl FnMut(usize, usize, usize) -> Option<()>,
) -> Option<()> {
    let mut base = 0usize;
    while base + DIRECTORY_HEADER_BYTES <= bytes.len() {
        if bytes.get(base..base + 4)? != GC_MAP_MAGIC {
            // Linkers pad between input sections; a zero tail is the end.
            if bytes[base..].iter().all(|byte| *byte == 0) {
                break;
            }
            base += 1;
            continue;
        }
        if read_u8(bytes, base + 4)? != GC_MAP_VERSION {
            return None;
        }
        // No header flags are defined; anything set is a layout this decoder
        // does not know. Fail closed.
        if read_u16(bytes, base + 6)? != 0 {
            return None;
        }
        let function_count = read_u32(bytes, base + 8)? as usize;
        let total_len = read_u32(bytes, base + 12)? as usize;
        // A directory must at least cover its header. Without this a
        // `total_len` of 0 leaves `base` unchanged — and because the magic
        // still matches at that offset the resynchronisation path above is
        // never reached, so the loop spins forever inside
        // `OnceLock::get_or_init`: a process hang at the first collection
        // rather than a fail-closed panic.
        if total_len < DIRECTORY_HEADER_BYTES {
            return None;
        }
        let blob_end = base.checked_add(total_len)?;
        if blob_end > bytes.len() {
            return None;
        }
        visit(base, function_count, total_len)?;
        let next = align_up(blob_end, 8)?;
        if next <= base {
            return None;
        }
        base = next;
    }
    Some(())
}

/// v6 (#11508): a function field is a signed 32-bit offset from the first byte
/// of its own blob, resolved by the linker (`.long fn-_perry_gc_map`) so the
/// section needs no load-time relocations. `origin` is the runtime address of
/// the section slice's first byte and `blob` the blob's offset within it.
pub(super) fn function_address(origin: usize, blob: usize, field: u32) -> Option<usize> {
    origin
        .checked_add(blob)?
        .checked_add_signed(field as i32 as isize)
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value & !(alignment - 1))
}

fn read_u8(bytes: &[u8], offset: usize) -> Option<u8> {
    bytes.get(offset).copied()
}

/// Used by the map header's flags field and by ELF section headers. It was
/// briefly Linux-gated, which broke the Linux build the moment the map itself
/// needed a 16-bit read — keep it unconditional.
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

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
/// The linker concatenates one blob per object file, so this walks blob by
/// blob using each header's `total_len` rather than assuming a single map —
/// a decoder that reads only the first header silently drops every other
/// object's roots, which is invisible until a collection frees a live object.
pub(super) fn parse_gc_map(
    bytes: &[u8],
) -> Option<(
    Vec<StackMapRecord>,
    Vec<StackMapLocation>,
    Vec<StackMapDerived>,
)> {
    let mut records = Vec::new();
    let mut roots: Vec<StackMapLocation> = Vec::new();
    let mut derived: Vec<StackMapDerived> = Vec::new();
    let mut base = 0usize;

    while base + 16 <= bytes.len() {
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
        let function_count = read_u32(bytes, base + 8)? as usize;
        let total_len = read_u32(bytes, base + 12)? as usize;
        // Header flags, bit 0: the function-address field is 8 bytes wide. The
        // emitter writes the TARGET's pointer width (watchOS `arm64_32` is
        // ILP32), and compile target and run target are the same machine — so
        // a mismatch here means the binary's map was produced for a different
        // width and every function address would be misread. Fail closed.
        let flags = read_u16(bytes, base + 6)?;
        if (flags & 1 == 1) != (std::mem::size_of::<usize>() == 8) {
            return None;
        }
        let entry = if flags & 1 == 1 { 16 } else { 12 };
        // A blob must at least cover its header, function table and v5
        // stream-offset array. Without this, a `total_len` of 0 leaves `base`
        // unchanged — and because the magic still matches at that offset the
        // resynchronisation path above is never reached, so the loop spins
        // forever. This runs inside `OnceLock::get_or_init`, so that is a
        // process hang at the first collection rather than a fail-closed panic.
        let table_bytes = function_count.checked_mul(entry)?;
        let stream_offset_bytes = function_count.checked_mul(4)?;
        if total_len
            < 16usize
                .checked_add(table_bytes)?
                .checked_add(stream_offset_bytes)?
        {
            return None;
        }
        let table = base.checked_add(16)?;
        let stream_offsets = table.checked_add(table_bytes)?;
        let instruction_offsets = stream_offsets.checked_add(stream_offset_bytes)?;
        let blob_end = base.checked_add(total_len)?;
        if blob_end > bytes.len() || instruction_offsets > blob_end {
            return None;
        }

        // Instruction offsets are a fixed-width array ahead of the varint
        // stream: at -O3 the compiler emits them as label differences the
        // assembler evaluates, so their values cannot be varint-encoded at
        // rewrite time.
        // Not `unwrap_or(0)`: a failed read here means the function table is
        // truncated, and treating that function as having zero records starts
        // the stream at the wrong offset so every later varint decodes from
        // misaligned bytes. A wrong live set is worse than no map.
        let mut record_total: usize = 0;
        for index in 0..function_count {
            record_total = record_total
                .checked_add(read_u32(bytes, table + index * entry + (entry - 4))? as usize)?;
        }
        let stream_base = instruction_offsets.checked_add(record_total.checked_mul(4)?)?;
        if stream_base > blob_end {
            return None;
        }

        let mut walk = RecordWalk::new(bytes, blob_end, stream_base, 0);
        let mut record_index = 0usize;

        for index in 0..function_count {
            // Address width follows the header flag checked above, so the
            // stack-size and record-count offsets move with it.
            let base_off = table + index * entry;
            let addr_bytes = entry - 8;
            let function_address = if addr_bytes == 8 {
                read_u64(bytes, base_off)? as usize
            } else {
                read_u32(bytes, base_off)? as usize
            };
            let stack_size = u64::from(read_u32(bytes, base_off + addr_bytes)?);
            let record_count = read_u32(bytes, base_off + addr_bytes + 4)?;

            // v5: the recorded per-function offset must BE where the
            // sequential walk stands. The compiler proves this for every
            // function of every binary it emits (`verify_roundtrip`); proving
            // it again here, against the shipped bytes, is what lets the lazy
            // lookup start a walk at a recorded offset and trust the result.
            let recorded = read_u32(bytes, stream_offsets + index * 4)? as usize;
            if stream_base.checked_add(recorded)? != walk.cursor() {
                return None;
            }

            // The repeat chain is per FUNCTION: the first record of a function
            // can never be a repeat, because the encoder re-arms there too.
            walk.restart(record_count);

            let mut previous: Option<(Payload, (u32, u32, u32, u32))> = None;
            for _ in 0..record_count {
                let instruction_offset = read_u32(bytes, instruction_offsets + record_index * 4)?;
                record_index += 1;

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
                            bytes,
                            payload.roots_at,
                            blob_end,
                            payload.roots_len,
                            &mut roots,
                        )?;
                        let derived_start = u32::try_from(derived.len()).ok()?;
                        collect_derived(bytes, &payload, blob_end, &mut derived)?;
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

        let next = align_up(blob_end, 8)?;
        if next <= base {
            return None;
        }
        base = next;
    }

    Some((records, roots, derived))
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

pub(super) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

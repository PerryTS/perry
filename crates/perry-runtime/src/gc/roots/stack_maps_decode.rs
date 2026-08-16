//! Compact GC-map decoding: the concatenated-section parser and the
//! byte-level primitives it reads through.
//!
//! Its own file for the same reason `stack_maps_verify.rs` is: the parent is at
//! the 2000-line cap, and pure code motion is the cheapest way to stay under it.

use super::{
    StackMapDerived, StackMapLocation, StackMapRecord, DWARF_REG_FP_AARCH64, DWARF_REG_SP_AARCH64,
    GC_MAP_MAGIC, GC_MAP_VERSION,
};

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
        // A blob must at least cover its header and function table. Without
        // this, a `total_len` of 0 leaves `base` unchanged — and because the
        // magic still matches at that offset the resynchronisation path below
        // is never reached, so the loop spins forever. This runs inside
        // `OnceLock::get_or_init`, so that is a process hang at the first
        // collection rather than the fail-closed panic in `stack_maps`.
        if total_len < 16 + function_count.checked_mul(entry)? {
            return None;
        }
        let table = base.checked_add(16)?;
        let stream_start = table.checked_add(function_count.checked_mul(entry)?)?;
        let blob_end = base.checked_add(total_len)?;
        if blob_end > bytes.len() || stream_start > blob_end {
            return None;
        }

        // Instruction offsets are a fixed-width array ahead of the varint
        // stream: at -O3 the compiler emits them as label differences the
        // assembler evaluates, so their values cannot be varint-encoded at
        // rewrite time.
        // Not `unwrap_or(0)`: a failed read here means the function table is
        // truncated, and treating that function as having zero records starts
        // `cursor` at the wrong offset so every later varint decodes from
        // misaligned bytes. A wrong live set is worse than no map.
        let mut record_total: usize = 0;
        for index in 0..function_count {
            record_total =
                record_total.checked_add(read_u32(bytes, table + index * 16 + 12)? as usize)?;
        }
        let offsets = stream_start;
        let mut cursor = offsets.checked_add(record_total.checked_mul(4)?)?;
        if cursor > blob_end {
            return None;
        }
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
            let record_count = read_u32(bytes, base_off + addr_bytes + 4)? as usize;

            // The shared tag/delta slot decoding (see gc_map.rs's
            // `encode_slots`): 2-bit base tag — 0 = FP, 1 = SP, 2 = explicit
            // DWARF register in a following varint (LLVM uses x19 as a frame
            // base in functions with dynamic allocation) — then a zigzagged
            // offset delta, chained per list.
            fn decode_slot_list(
                bytes: &[u8],
                mut cursor: usize,
                blob_end: usize,
                count: usize,
                out: &mut Vec<StackMapLocation>,
            ) -> Option<usize> {
                let mut last: Option<i32> = None;
                for _ in 0..count {
                    let (value, next) = read_varint(bytes, cursor, blob_end)?;
                    cursor = next;
                    let dwarf_reg = match value & 3 {
                        0 => DWARF_REG_FP_AARCH64,
                        1 => DWARF_REG_SP_AARCH64,
                        2 => {
                            let (reg, next) = read_varint(bytes, cursor, blob_end)?;
                            cursor = next;
                            u16::try_from(reg).ok()?
                        }
                        _ => return None,
                    };
                    let delta = unzigzag((value >> 2) as u32);
                    let offset = match last {
                        None => delta,
                        Some(previous_offset) => previous_offset.wrapping_add(delta),
                    };
                    last = Some(offset);
                    out.push(StackMapLocation { dwarf_reg, offset });
                }
                Some(cursor)
            }

            let mut previous: Option<(u32, u32, u32, u32)> = None;
            for _ in 0..record_count {
                let instruction_offset = read_u32(bytes, offsets + record_index * 4)?;
                record_index += 1;

                let (header, next) = read_varint(bytes, cursor, blob_end)?;
                cursor = next;
                let range = if header & 1 == 1 {
                    // Repeat: this safepoint's live set is the previous one's
                    // — bases and deriveds both.
                    previous?
                } else {
                    // v4 header word: (root_count << 2) | (has_derived << 1).
                    let count = (header >> 2) as usize;
                    let has_derived = header & 2 != 0;
                    let start = u32::try_from(roots.len()).ok()?;
                    cursor = decode_slot_list(bytes, cursor, blob_end, count, &mut roots)?;
                    let derived_start = u32::try_from(derived.len()).ok()?;
                    let mut derived_count = 0u32;
                    if has_derived {
                        let (entries, next) = read_varint(bytes, cursor, blob_end)?;
                        cursor = next;
                        derived_count = u32::try_from(entries).ok()?;
                        let mut bases = Vec::with_capacity(derived_count as usize);
                        for _ in 0..derived_count {
                            let (base_index, next) = read_varint(bytes, cursor, blob_end)?;
                            cursor = next;
                            // The base index addresses THIS record's roots
                            // list; out of range means the stream and this
                            // decoder disagree — fail closed like any other
                            // malformed map.
                            if base_index >= count as u64 {
                                return None;
                            }
                            bases.push(u32::try_from(base_index).ok()?);
                        }
                        let mut slots = Vec::with_capacity(derived_count as usize);
                        cursor = decode_slot_list(
                            bytes,
                            cursor,
                            blob_end,
                            derived_count as usize,
                            &mut slots,
                        )?;
                        for (base_index, slot) in bases.into_iter().zip(slots) {
                            derived.push(StackMapDerived { base_index, slot });
                        }
                    }
                    (
                        start,
                        u32::try_from(count).ok()?,
                        derived_start,
                        derived_count,
                    )
                };
                previous = Some(range);

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

/// LEB128 read bounded by the blob it belongs to, so a corrupt length cannot
/// walk into the next blob or off the section.
fn read_varint(bytes: &[u8], mut at: usize, end: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        if at >= end || shift > 63 {
            return None;
        }
        let byte = *bytes.get(at)?;
        at += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
        shift += 7;
    }
}

fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
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

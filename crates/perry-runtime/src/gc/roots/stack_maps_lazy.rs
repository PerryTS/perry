//! The per-function view of the compact GC map: the function table the index
//! is built from, and the one record-stream decoder both the lazy lookup and
//! the eager parser drive.
//!
//! # Why this exists
//!
//! The v4 map's record stream is varint-chained, so reaching function `i`'s
//! records means decoding functions `0..i` first. The only way to answer "what
//! is live in THIS frame" was therefore to decode the whole section at
//! startup: for claude-code, 22.7 MB and 2,078,970 records into a ~117 MB
//! index — to answer the **74 record lookups** a `--help` run actually makes.
//!
//! v5 adds one array, `u32 stream_offset` per function, so a function's
//! records can be found without decoding any other function's. That is +1.3%
//! of the section and it is the whole format change.
//!
//! # Why there is exactly one decoder in this file
//!
//! This is GC root metadata: a wrong answer frees live objects with no
//! diagnostic. The safety argument is not "the lazy path and the eager path
//! agree today", it is that **they cannot disagree** — [`RecordWalk`] is the
//! only thing that advances through the record stream, and `parse_gc_map`
//! drives it over a whole blob while [`MatchedRecords`] drives it over one
//! function. The difference between the two is where the cursor starts and
//! nothing else. Do not add a second walk here; make this one serve both.

use super::decode::read_u32;
use super::{StackMapDerived, StackMapLocation, DWARF_REG_FP_AARCH64, DWARF_REG_SP_AARCH64};

/// One function's position in the map, in the form the root scan needs.
///
/// 24 bytes, one per function with records: 1.7 MB for claude-code's 72,669,
/// against the ~117 MB of materialised records it replaces. Byte offsets are
/// relative to the function's records blob rather than pointers, so the entry
/// stays small and a stale entry cannot be dereferenced without going through
/// `records[blob]`, whose length is also the bound every varint read in this
/// function is checked against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FunctionEntry {
    /// Relocated function address, from the map's directory.
    pub(super) address: usize,
    /// Offset of this function's first instruction offset in its records
    /// blob's fixed-width `u32` array.
    pub(super) offsets_at: u32,
    /// Offset of this function's first record header in its records blob.
    pub(super) stream_at: u32,
    pub(super) record_count: u32,
    /// Index into the index's records-blob list.
    pub(super) blob: u32,
}

/// Where one record's live set lives in the stream.
///
/// A repeat record carries the PREVIOUS record's payload, which is how 63.1%
/// of claude-code's records cost one byte each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Payload {
    /// Cursor of the first root slot's varint.
    pub(super) roots_at: usize,
    pub(super) roots_len: u32,
    /// Cursor of the first derived base-index varint. Meaningless when
    /// `derived_len` is 0.
    pub(super) derived_bases_at: usize,
    pub(super) derived_len: u32,
}

/// One step of [`RecordWalk`].
pub(super) enum Step {
    Record(Payload),
    Done,
    /// The stream and this decoder disagree. Every caller fails closed: a
    /// wrong live set is worse than no map, and worse than a crash.
    Malformed,
}

/// The one decoder that advances through the record stream.
///
/// `parse_gc_map` drives it over every function of a blob in order; the lazy
/// lookup drives it over a single function starting at that function's
/// `stream_offset`. Both see the same bytes through the same state machine, so
/// "the lazy path decodes differently" is not expressible.
pub(super) struct RecordWalk<'a> {
    bytes: &'a [u8],
    blob_end: usize,
    cursor: usize,
    previous: Option<Payload>,
    remaining: u32,
}

impl<'a> RecordWalk<'a> {
    pub(super) fn new(bytes: &'a [u8], blob_end: usize, stream_at: usize, count: u32) -> Self {
        Self {
            bytes,
            blob_end,
            cursor: stream_at,
            previous: None,
            remaining: count,
        }
    }

    /// The cursor is not reset between functions by the caller that walks a
    /// whole blob; `parse_gc_map` re-arms per function so the repeat chain
    /// cannot cross a function boundary, exactly as the v4 decoder did (its
    /// `previous` was declared inside the function loop).
    pub(super) fn restart(&mut self, count: u32) {
        self.previous = None;
        self.remaining = count;
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn next(&mut self) -> Step {
        if self.remaining == 0 {
            return Step::Done;
        }
        self.remaining -= 1;
        let Some((header, next)) = read_varint(self.bytes, self.cursor, self.blob_end) else {
            return Step::Malformed;
        };
        self.cursor = next;
        if header & 1 == 1 {
            // Repeat: this safepoint's live set is the previous one's, bases
            // and deriveds both. A repeat with no previous record is a
            // malformed stream, not an empty live set.
            return match self.previous {
                Some(payload) => Step::Record(payload),
                None => Step::Malformed,
            };
        }
        // v4/v5 header word: (root_count << 2) | (has_derived << 1).
        let roots_len = (header >> 2) as u32;
        let has_derived = header & 2 != 0;
        let roots_at = self.cursor;
        let Some(after_roots) = skip_slots(self.bytes, self.cursor, self.blob_end, roots_len)
        else {
            return Step::Malformed;
        };
        self.cursor = after_roots;
        let mut derived_bases_at = 0usize;
        let mut derived_len = 0u32;
        if has_derived {
            let Some((count, next)) = read_varint(self.bytes, self.cursor, self.blob_end) else {
                return Step::Malformed;
            };
            let Ok(count) = u32::try_from(count) else {
                return Step::Malformed;
            };
            derived_len = count;
            derived_bases_at = next;
            let mut cursor = next;
            for _ in 0..count {
                // The base index addresses THIS record's roots list; out of
                // range means the stream and this decoder disagree.
                let Some((base_index, next)) = read_varint(self.bytes, cursor, self.blob_end)
                else {
                    return Step::Malformed;
                };
                if base_index >= u64::from(roots_len) {
                    return Step::Malformed;
                }
                cursor = next;
            }
            let Some(after_derived) = skip_slots(self.bytes, cursor, self.blob_end, count) else {
                return Step::Malformed;
            };
            self.cursor = after_derived;
        }
        let payload = Payload {
            roots_at,
            roots_len,
            derived_bases_at,
            derived_len,
        };
        self.previous = Some(payload);
        Step::Record(payload)
    }
}

/// Decode a slot list, one slot at a time, with no buffering.
///
/// The shared tag/delta encoding (see `gc_map.rs`'s `encode_slots`): a 2-bit
/// base tag — 0 = FP, 1 = SP, 2 = explicit DWARF register in a following
/// varint (LLVM uses x19 as a frame base in functions with dynamic
/// allocation) — then a zigzagged offset delta, chained per list.
pub(super) struct SlotIter<'a> {
    bytes: &'a [u8],
    cursor: usize,
    end: usize,
    remaining: u32,
    last: Option<i32>,
    range: Option<(u16, i32, u32)>,
}

impl<'a> SlotIter<'a> {
    pub(super) fn new(bytes: &'a [u8], cursor: usize, end: usize, count: u32) -> Self {
        Self {
            bytes,
            cursor,
            end,
            remaining: count,
            last: None,
            range: None,
        }
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    /// `None` ends the list; `Some(None)` is a malformed stream, which every
    /// caller turns into a fail-closed abort rather than a short live set.
    #[allow(clippy::should_implement_trait)]
    pub(super) fn next(&mut self) -> Option<Option<StackMapLocation>> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        if let Some((reg, offset, words)) = self.range {
            self.last = Some(offset);
            self.range = if words > 1 {
                Some((reg, offset + 8, words - 1))
            } else {
                None
            };
            return Some(Some(StackMapLocation {
                dwarf_reg: reg,
                offset,
            }));
        }
        let Some((value, next)) = read_varint(self.bytes, self.cursor, self.end) else {
            return Some(None);
        };
        self.cursor = next;
        let dwarf_reg = match value & 3 {
            0 => DWARF_REG_FP_AARCH64,
            1 => DWARF_REG_SP_AARCH64,
            2 => {
                let Some((reg, next)) = read_varint(self.bytes, self.cursor, self.end) else {
                    return Some(None);
                };
                self.cursor = next;
                match u16::try_from(reg) {
                    Ok(reg) => reg,
                    Err(_) => return Some(None),
                }
            }
            3 => {
                let Some((reg, next)) = read_varint(self.bytes, self.cursor, self.end) else {
                    return Some(None);
                };
                let Some((words, after)) = read_varint(self.bytes, next, self.end) else {
                    return Some(None);
                };
                let (Ok(reg), Ok(words)) = (u16::try_from(reg), u32::try_from(words)) else {
                    return Some(None);
                };
                if words == 0 || words - 1 > self.remaining {
                    return Some(None);
                }
                self.cursor = after;
                let delta = unzigzag((value >> 2) as u32);
                let offset = self.last.map_or(delta, |last| last.wrapping_add(delta));
                let Some(extent) = (words - 1)
                    .checked_mul(8)
                    .and_then(|n| i32::try_from(n).ok())
                else {
                    return Some(None);
                };
                if offset.checked_add(extent).is_none() {
                    return Some(None);
                }
                self.last = Some(offset);
                if words > 1 {
                    self.range = Some((reg, offset + 8, words - 1));
                }
                return Some(Some(StackMapLocation {
                    dwarf_reg: reg,
                    offset,
                }));
            }
            _ => return Some(None),
        };
        let delta = unzigzag((value >> 2) as u32);
        let offset = match self.last {
            None => delta,
            Some(previous) => previous.wrapping_add(delta),
        };
        self.last = Some(offset);
        Some(Some(StackMapLocation { dwarf_reg, offset }))
    }
}

/// Collect a whole slot list. Used by the eager parser and by the cross-check;
/// the walk itself never buffers.
pub(super) fn collect_slots(
    bytes: &[u8],
    cursor: usize,
    end: usize,
    count: u32,
    out: &mut Vec<StackMapLocation>,
) -> Option<usize> {
    let mut iter = SlotIter::new(bytes, cursor, end, count);
    while let Some(slot) = iter.next() {
        out.push(slot?);
    }
    Some(iter.cursor())
}

/// The derived entries of one record: base index paired with its slot.
pub(super) fn collect_derived(
    bytes: &[u8],
    payload: &Payload,
    end: usize,
    out: &mut Vec<StackMapDerived>,
) -> Option<()> {
    if payload.derived_len == 0 {
        return Some(());
    }
    let mut cursor = payload.derived_bases_at;
    let mut bases = Vec::with_capacity(payload.derived_len as usize);
    for _ in 0..payload.derived_len {
        let (base_index, next) = read_varint(bytes, cursor, end)?;
        cursor = next;
        bases.push(u32::try_from(base_index).ok()?);
    }
    let mut slots = Vec::with_capacity(payload.derived_len as usize);
    collect_slots(bytes, cursor, end, payload.derived_len, &mut slots)?;
    for (base_index, slot) in bases.into_iter().zip(slots) {
        out.push(StackMapDerived { base_index, slot });
    }
    Some(())
}

/// Cursor of the first derived SLOT, i.e. past the base-index varints.
pub(super) fn derived_slots_at(bytes: &[u8], payload: &Payload, end: usize) -> Option<usize> {
    let mut cursor = payload.derived_bases_at;
    for _ in 0..payload.derived_len {
        let (_, next) = read_varint(bytes, cursor, end)?;
        cursor = next;
    }
    Some(cursor)
}

fn skip_slots(bytes: &[u8], mut cursor: usize, end: usize, mut count: u32) -> Option<usize> {
    let mut last: Option<i32> = None;
    while count != 0 {
        let (value, next) = read_varint(bytes, cursor, end)?;
        cursor = next;
        let mut words = 1;
        if value & 3 >= 2 {
            let (reg, next) = read_varint(bytes, cursor, end)?;
            u16::try_from(reg).ok()?;
            cursor = next;
            if value & 3 == 3 {
                let (n, next) = read_varint(bytes, cursor, end)?;
                words = u32::try_from(n).ok()?;
                cursor = next;
            }
        }
        if words == 0 || words > count {
            return None;
        }
        let delta = unzigzag((value >> 2) as u32);
        let offset = last.map_or(delta, |old| old.wrapping_add(delta));
        let extent = i32::try_from((words - 1).checked_mul(8)?).ok()?;
        last = Some(offset.checked_add(extent)?);
        count -= words;
    }
    Some(cursor)
}

/// LEB128 read bounded by the blob it belongs to, so a corrupt length cannot
/// walk into the next blob or off the section.
pub(super) fn read_varint(bytes: &[u8], mut at: usize, end: usize) -> Option<(u64, usize)> {
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

pub(super) fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

/// Walk every directory and append one [`FunctionEntry`] per function that
/// has records, and one records blob per directory.
///
/// This is the whole of what a lazy build reads: v8 directories, 1.4 MB of
/// claude-code's map, against the 22.7 MB read and ~117 MB written by the
/// eager build. The records themselves are not touched until a walk names a
/// frame of theirs.
///
/// `origin` is the runtime address of `bytes[0]`; the v6 function fields are
/// offsets from their blob, so the table cannot be read without it.
pub(super) fn parse_function_table<'a>(
    bytes: &'a [u8],
    origin: usize,
    out: &mut Vec<FunctionEntry>,
    blobs: &mut Vec<&'a [u8]>,
) -> Option<()> {
    super::decode::for_each_directory(bytes, |directory| {
        let blob = u32::try_from(blobs.len()).ok()?;
        blobs.push(directory.records);
        let stream_base = directory.stream_base();
        for index in 0..directory.function_count {
            let function = directory.function(bytes, origin, index)?;
            // A function with no records contributes nothing and MUST NOT
            // enter the table: it would sit between two real functions and
            // shadow the containing one for every `ip` inside it — a wrong
            // (empty) live set rather than a missing entry. The v4 index
            // derived its function list from records and so excluded these by
            // construction; excluding them here keeps that property.
            if function.record_count == 0 {
                continue;
            }
            out.push(FunctionEntry {
                address: function.address,
                offsets_at: u32::try_from(function.first_record.checked_mul(4)?).ok()?,
                stream_at: u32::try_from(stream_base.checked_add(function.stream_offset)?).ok()?,
                record_count: function.record_count,
                blob,
            });
        }
        Some(())
    })
}

/// One record's live set, as positions in the section rather than as a
/// materialised list. Decoding happens when the walker asks, and only for the
/// handful of records a stack walk actually names.
#[derive(Clone, Copy)]
pub(super) struct DecodedRecord {
    pub(super) bytes: &'static [u8],
    pub(super) blob_end: usize,
    pub(super) function_address: usize,
    pub(super) payload: Payload,
}

impl DecodedRecord {
    pub(super) fn roots(&self) -> SlotIter<'static> {
        SlotIter::new(
            self.bytes,
            self.payload.roots_at,
            self.blob_end,
            self.payload.roots_len,
        )
    }

    pub(super) fn derived_slots(&self) -> Option<SlotIter<'static>> {
        if self.payload.derived_len == 0 {
            return Some(SlotIter::new(self.bytes, 0, 0, 0));
        }
        let cursor = derived_slots_at(self.bytes, &self.payload, self.blob_end)?;
        Some(SlotIter::new(
            self.bytes,
            cursor,
            self.blob_end,
            self.payload.derived_len,
        ))
    }

    /// Base index of each derived slot, in the same order `derived_slots`
    /// yields them.
    pub(super) fn derived_base_indices(&self) -> SlotBaseIter {
        SlotBaseIter {
            bytes: self.bytes,
            cursor: self.payload.derived_bases_at,
            end: self.blob_end,
            remaining: self.payload.derived_len,
        }
    }
}

pub(super) struct SlotBaseIter {
    bytes: &'static [u8],
    cursor: usize,
    end: usize,
    remaining: u32,
}

impl SlotBaseIter {
    #[allow(clippy::should_implement_trait)]
    pub(super) fn next(&mut self) -> Option<Option<u32>> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let Some((value, next)) = read_varint(self.bytes, self.cursor, self.end) else {
            return Some(None);
        };
        self.cursor = next;
        Some(u32::try_from(value).ok())
    }
}

/// The records describing the frame whose return address is `ip`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RecordMatch {
    /// Every function-table entry sharing the owning function's address.
    ///
    /// A RANGE, not an index. Two entries can carry the same relocated
    /// address — a symbol emitted by more than one object file, or code the
    /// linker folded — and each brings its own records. Taking only one would
    /// drop the other's roots silently, so the match is the whole run.
    pub(super) functions: core::ops::Range<usize>,
    /// The instruction offset every matched record carries.
    pub(super) offset: u32,
}

/// Find the records for `ip`: the nearest-offset match inside the function
/// that contains `ip`.
///
/// This differs from the v4 lookup in one way worth stating. v4 searched every
/// record in the image for the globally nearest pc and THEN required it to
/// belong to the function containing `ip`, so a nearer record in a neighbouring
/// function suppressed a legitimate match. Searching inside the owning function
/// from the start cannot do that. The old comment recorded that every near-match
/// in the probe suite was already same-function, so this is the same answer in
/// practice — and where it differs it returns the roots of the function `ip` is
/// actually executing, which is the contract the containment check was written
/// to express.
pub(super) fn match_records(
    functions: &[FunctionEntry],
    blobs: &[&'static [u8]],
    ip: usize,
    max_delta: usize,
) -> Option<RecordMatch> {
    let at = functions
        .partition_point(|entry| entry.address <= ip)
        .checked_sub(1)?;
    let address = functions[at].address;
    let start = functions.partition_point(|entry| entry.address < address);
    let end = functions.partition_point(|entry| entry.address <= address);
    let target = u32::try_from(ip.checked_sub(address)?).ok()?;

    let mut best: Option<(u32, u32)> = None; // (distance, offset)
    for entry in &functions[start..end] {
        let bytes = *blobs.get(entry.blob as usize)?;
        for index in 0..entry.record_count {
            let at = entry.offsets_at as usize + index as usize * 4;
            let Some(offset) = read_u32(bytes, at) else {
                continue;
            };
            let distance = offset.abs_diff(target);
            if best.is_none_or(|(best_distance, _)| distance < best_distance) {
                best = Some((distance, offset));
            }
        }
    }
    let (distance, offset) = best?;
    if distance as usize > max_delta {
        return None;
    }
    Some(RecordMatch {
        functions: start..end,
        offset,
    })
}

/// Decode the matched records, one at a time.
///
/// Hand-rolled rather than an `Iterator` so a caller can `return` out of the
/// loop — the fp-chain walker fails closed to the platform unwinder from
/// inside it — without the borrow of the index outliving the body.
pub(super) struct MatchedRecords<'a> {
    functions: &'a [FunctionEntry],
    blobs: &'a [&'static [u8]],
    range: core::ops::Range<usize>,
    offset: u32,
    walk: Option<(usize, RecordWalk<'static>, u32)>,
}

impl<'a> MatchedRecords<'a> {
    pub(super) fn new(
        functions: &'a [FunctionEntry],
        blobs: &'a [&'static [u8]],
        matched: &RecordMatch,
    ) -> Self {
        Self {
            functions,
            blobs,
            range: matched.functions.clone(),
            offset: matched.offset,
            walk: None,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub(super) fn next(&mut self) -> Option<DecodedRecord> {
        loop {
            if self.walk.is_none() {
                let index = self.range.next()?;
                let entry = self.functions.get(index)?;
                let bytes = *self.blobs.get(entry.blob as usize)?;
                self.walk = Some((
                    index,
                    RecordWalk::new(
                        bytes,
                        bytes.len(),
                        entry.stream_at as usize,
                        entry.record_count,
                    ),
                    0,
                ));
            }
            let (index, walk, record) = self.walk.as_mut()?;
            let entry = self.functions.get(*index)?;
            let bytes = *self.blobs.get(entry.blob as usize)?;
            match walk.next() {
                Step::Record(payload) => {
                    let at = entry.offsets_at as usize + *record as usize * 4;
                    let offset = read_u32(bytes, at);
                    *record += 1;
                    if offset == Some(self.offset) {
                        return Some(DecodedRecord {
                            bytes,
                            blob_end: bytes.len(),
                            function_address: entry.address,
                            payload,
                        });
                    }
                }
                Step::Done => self.walk = None,
                Step::Malformed => malformed(entry.address),
            }
        }
    }
}

/// A record stream that does not decode is not "no roots".
///
/// The eager parser turns this into a panic at index-build time; reaching it
/// here means the same thing one step later, and it must mean the same thing:
/// continuing would hand the collector a truncated live set and free live
/// objects with no diagnostic. Every check the build performs — magic,
/// version, pointer width, table bounds, stream-offset monotonicity — still
/// runs eagerly, so this can only be reached by corruption inside the record
/// stream itself.
#[cold]
pub(super) fn malformed(function_address: usize) -> ! {
    panic!(
        "perry: the GC map's record stream for the function at {function_address:#x} does not \
         decode. The compiler verifies this stream against the recorded stack map on every \
         compile, so this binary's map has been corrupted since it was produced; continuing \
         would scan a truncated live set and free live objects silently."
    );
}

/// Build a v8 blob, mirroring `perry-codegen/src/gc_map.rs`.
///
/// Shared by every test that needs real section bytes rather than hand-made
/// records — which, since the walkers decode from the section, is all of them.
#[cfg(test)]
pub(super) type TestFunction = (u64, u32, Vec<(u32, Vec<(u16, i32)>)>);

#[cfg(test)]
pub(super) fn test_blob(
    address: u64,
    stack_size: u32,
    records: &[(u32, Vec<(u16, i32)>)],
) -> Vec<u8> {
    test_blob_multi(&[(address, stack_size, records.to_vec())])
}

/// A blob read at origin 0 (what `build_index_from_sections_lazy` passes), so
/// each function field is simply its address — which must then fit in an i32.
#[cfg(test)]
pub(super) fn test_blob_multi(functions: &[TestFunction]) -> Vec<u8> {
    test_blob_multi_at(0, functions)
}

/// A blob whose function fields are relative to `origin`: the address the
/// decoder will be told the blob's first byte lives at.
#[cfg(test)]
pub(super) fn test_blob_multi_at(origin: u64, functions: &[TestFunction]) -> Vec<u8> {
    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7F) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }
    fn zigzag(value: i32) -> u64 {
        ((value << 1) ^ (value >> 31)) as u32 as u64
    }

    let mut offsets = Vec::new();
    let mut stream = Vec::new();
    let mut stream_offsets = Vec::new();
    for (_, _, records) in functions {
        stream_offsets.push(stream.len() as u32);
        for (instruction_offset, roots) in records {
            offsets.extend_from_slice(&instruction_offset.to_le_bytes());
            push_varint(&mut stream, (roots.len() as u64) << 2);
            let mut last: Option<i32> = None;
            for (reg, offset) in roots {
                let tag = match *reg {
                    DWARF_REG_FP_AARCH64 => 0u64,
                    DWARF_REG_SP_AARCH64 => 1,
                    _ => 2,
                };
                let delta = match last {
                    None => *offset,
                    Some(previous) => offset.wrapping_sub(previous),
                };
                push_varint(&mut stream, (zigzag(delta) << 2) | tag);
                if tag == 2 {
                    push_varint(&mut stream, u64::from(*reg));
                }
                last = Some(*offset);
            }
        }
    }

    // v8, in the in-line form: the records follow the directory inside the
    // blob's stride, so a test section stays one self-contained byte string
    // that can be concatenated like linker input sections. The compiler puts
    // them in a section of their own; the decoder reads both the same way,
    // through the header's self-relative `records_offset`.
    let header = super::decode::DIRECTORY_HEADER_BYTES;
    let entry = super::decode::FUNCTION_ENTRY_BYTES;
    let directory_len = header + functions.len() * entry;
    let records_len = offsets.len() + stream.len();
    let record_total = offsets.len() / 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(super::GC_MAP_MAGIC);
    bytes.push(super::GC_MAP_VERSION);
    bytes.push(0);
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(functions.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&((directory_len + records_len) as u32).to_le_bytes());
    bytes.extend_from_slice(&(directory_len as i32).to_le_bytes());
    bytes.extend_from_slice(&(records_len as u32).to_le_bytes());
    bytes.extend_from_slice(&(record_total as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let mut first_record = 0u32;
    for ((address, stack_size, records), stream_offset) in functions.iter().zip(&stream_offsets) {
        let offset = i32::try_from(address.wrapping_sub(origin) as i64)
            .expect("a test function must sit within +-2 GiB of its blob");
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&stack_size.to_le_bytes());
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&first_record.to_le_bytes());
        bytes.extend_from_slice(&stream_offset.to_le_bytes());
        first_record += records.len() as u32;
    }
    bytes.extend_from_slice(&offsets);
    bytes.extend_from_slice(&stream);
    while bytes.len() % 8 != 0 {
        bytes.push(0);
    }
    bytes
}

#[cfg(test)]
mod native_range_tests {
    use super::*;

    #[test]
    fn native_range_walks_without_materializing_slots() {
        // Range of three x86-64 RBP homes at -24, -16, -8.
        let bytes = [191, 1, 6, 3];
        assert_eq!(skip_slots(&bytes, 0, bytes.len(), 3), Some(bytes.len()));
        let mut iter = SlotIter::new(&bytes, 0, bytes.len(), 3);
        for offset in [-24, -16, -8] {
            assert_eq!(
                iter.next().unwrap().unwrap(),
                StackMapLocation {
                    dwarf_reg: 6,
                    offset
                }
            );
        }
        assert!(iter.next().is_none());
        assert_eq!(iter.cursor(), bytes.len());
    }

    #[test]
    fn malformed_native_ranges_fail_closed() {
        for (bytes, count) in [
            (vec![3, 6, 0], 1),           // empty
            (vec![3, 6, 3], 2),           // exceeds declared live set
            (vec![3, 6], 1),              // missing size
            (vec![3, 128, 128, 4, 1], 1), // register exceeds u16
        ] {
            assert!(skip_slots(&bytes, 0, bytes.len(), count).is_none());
            assert!(SlotIter::new(&bytes, 0, bytes.len(), count)
                .next()
                .unwrap()
                .is_none());
        }
    }
}

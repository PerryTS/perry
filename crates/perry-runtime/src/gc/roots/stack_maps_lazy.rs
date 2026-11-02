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
/// 32 bytes, one per function with records: 2.3 MB for claude-code's 72,669,
/// against the ~117 MB of materialised records it replaces. v9 gives every
/// function its own records blob, so the entry names it directly; its length
/// is the bound every varint read in this function is checked against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FunctionEntry {
    /// Relocated function address, from the map's entry.
    pub(super) address: usize,
    /// Address of this function's records blob: `record_count` u32
    /// instruction offsets, then its stream.
    pub(super) records: usize,
    pub(super) records_len: u32,
    pub(super) record_count: u32,
    /// The largest instruction offset of this function's records. An `ip`
    /// more than the match window beyond it matches nothing here, which
    /// [`match_records`] then says in one search instead of scanning every
    /// record offset. Derived from the records at build; with `stack_size`
    /// it fills what was padding.
    pub(super) max_offset: u32,
    /// The map's frame size for this function (`u32::MAX`: dynamic), for the
    /// x86-64 frame walk's coherence check.
    pub(super) stack_size: u32,
}

impl FunctionEntry {
    /// The function's records blob.
    pub(super) fn bytes(&self) -> &'static [u8] {
        // SAFETY: built from a validated entry of a loaded image's map (or a
        // test's leaked allocation); image sections stay mapped while their
        // code can be on a stack.
        unsafe { core::slice::from_raw_parts(self.records as *const u8, self.records_len as usize) }
    }

    /// Offset of the stream within [`Self::bytes`].
    pub(super) fn stream_at(&self) -> usize {
        self.record_count as usize * 4
    }
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

/// Walk every entry and append one [`FunctionEntry`] per function that has
/// records.
///
/// This is the whole of what a lazy build reads: the entries, 1.4 MB of
/// claude-code's map, against the 22.7 MB read and ~117 MB written by the
/// eager build. The records themselves are not touched until a walk names a
/// frame of theirs.
///
/// `origin` is the runtime address of `bytes[0]`; the v6 function fields are
/// offsets from their entry, so the table cannot be read without it.
pub(super) fn parse_function_table(
    bytes: &'static [u8],
    origin: usize,
    out: &mut Vec<FunctionEntry>,
) -> Option<()> {
    super::decode::for_each_entry(bytes, origin, |entry| {
        // A function with no records contributes nothing and MUST NOT enter
        // the table: it would sit between two real functions and shadow the
        // containing one for every `ip` inside it — a wrong (empty) live set
        // rather than a missing entry. The v4 index derived its function list
        // from records and so excluded these by construction; excluding them
        // here keeps that property.
        if entry.record_count == 0 {
            return Some(());
        }
        out.push(FunctionEntry {
            address: entry.address,
            records: entry.records.as_ptr() as usize,
            records_len: u32::try_from(entry.records.len()).ok()?,
            record_count: entry.record_count,
            max_offset: max_record_offset(entry.records.get(..entry.stream_at())?),
            stack_size: entry.stack_size,
        });
        Some(())
    })
}

/// [`FunctionEntry::max_offset`] of a function whose record offsets are the
/// little-endian `u32`s of `offsets`: the maximum, not the last, because
/// nothing in the format orders a function's records by offset. One
/// sequential pass the compiler vectorises.
fn max_record_offset(offsets: &[u8]) -> u32 {
    offsets
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .fold(0u32, u32::max)
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
    ip: usize,
    max_delta: usize,
) -> Option<RecordMatch> {
    // One search. Entries sharing an address are adjacent, so the run that
    // owns `ip` ends here and its start is a short step back (almost always
    // zero steps).
    let end = functions.partition_point(|entry| entry.address <= ip);
    let address = functions[end.checked_sub(1)?].address;
    let mut start = end - 1;
    while start > 0 && functions[start - 1].address == address {
        start -= 1;
    }
    let target = u32::try_from(ip.checked_sub(address)?).ok()?;
    // A pc past every record of the run matches nothing in it: a Rust or
    // libc frame above the nearest lower JS function, which used to scan
    // every one of that function's record offsets to learn the same thing.
    if functions[start..end]
        .iter()
        .all(|entry| target as usize > entry.max_offset as usize + max_delta)
    {
        return None;
    }

    let mut best: Option<(u32, u32)> = None; // (distance, offset)
    for entry in &functions[start..end] {
        let bytes = entry.bytes();
        for index in 0..entry.record_count {
            let at = index as usize * 4;
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
    range: core::ops::Range<usize>,
    offset: u32,
    walk: Option<(usize, RecordWalk<'static>, u32)>,
}

impl<'a> MatchedRecords<'a> {
    pub(super) fn new(functions: &'a [FunctionEntry], matched: &RecordMatch) -> Self {
        Self {
            functions,
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
                let bytes = entry.bytes();
                self.walk = Some((
                    index,
                    RecordWalk::new(bytes, bytes.len(), entry.stream_at(), entry.record_count),
                    0,
                ));
            }
            let (index, walk, record) = self.walk.as_mut()?;
            let entry = self.functions.get(*index)?;
            let bytes = entry.bytes();
            match walk.next() {
                Step::Record(payload) => {
                    let at = *record as usize * 4;
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

/// Build a v9 map section, mirroring `perry-codegen/src/gc_map.rs`.
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
) -> &'static [u8] {
    test_blob_multi(&[(address, stack_size, records.to_vec())])
}

/// A section read at origin 0 (what `build_index_from_sections_lazy` passes),
/// so each function field is its address less its entry's offset.
#[cfg(test)]
pub(super) fn test_blob_multi(functions: &[TestFunction]) -> &'static [u8] {
    test_blob_multi_at(0, functions)
}

/// A section whose function fields are relative to `origin`: the address the
/// decoder will be told the section's first byte lives at.
#[cfg(test)]
pub(super) fn test_blob_multi_at(origin: u64, functions: &[TestFunction]) -> &'static [u8] {
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
    let maps: Vec<(u64, u32, Vec<u32>, Vec<u8>)> = functions
        .iter()
        .map(|(address, stack_size, records)| {
            let mut stream = Vec::new();
            for (_, roots) in records {
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
            let offsets = records.iter().map(|(offset, _)| *offset).collect();
            (*address, *stack_size, offsets, stream)
        })
        .collect();
    test_section_at(origin, &maps)
}

/// The v9 section for `(address, stack_size, instruction offsets, stream)`
/// per function: the entries, as the linker concatenates them, with every
/// records blob after them in the same leaked allocation (the compiler puts
/// them in a section of their own; the decoder finds both through the
/// entry's self-relative offset). Only the entries are returned.
#[cfg(test)]
pub(super) fn test_section_at(
    origin: u64,
    maps: &[(u64, u32, Vec<u32>, Vec<u8>)],
) -> &'static [u8] {
    let entry = super::decode::FUNCTION_ENTRY_BYTES;
    let entries_len = maps.len() * entry;
    let mut bytes = vec![0u8; entries_len];
    for (index, (address, stack_size, offsets, stream)) in maps.iter().enumerate() {
        let at = index * entry;
        let records_at = bytes.len();
        for offset in offsets {
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(stream);
        let field = i32::try_from(address.wrapping_sub(origin + at as u64) as i64)
            .expect("a test function must sit within +-2 GiB of its entry");
        let counted = offsets.len() as u32 | u32::from(super::GC_MAP_VERSION) << 24;
        bytes[at..at + 4].copy_from_slice(&field.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&stack_size.to_le_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&counted.to_le_bytes());
        bytes[at + 12..at + 16].copy_from_slice(&((records_at - at) as i32).to_le_bytes());
        let records_len = (bytes.len() - records_at) as u32;
        bytes[at + 16..at + 20].copy_from_slice(&records_len.to_le_bytes());
    }
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    &leaked[..entries_len]
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

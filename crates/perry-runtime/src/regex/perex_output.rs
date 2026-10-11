//! The one output builder of every replacement (`replace`, `replaceAll`, both
//! string and RegExp patterns, template or callback replacements) and of the
//! other operations that concatenate known pieces.
//!
//! The output is built in native memory as the final string's own encoding
//! (WTF-8) and allocated once, at the end, as one heap string; a result of at
//! most `SHORT_STRING_MAX_LEN` ASCII units is an immediate and allocates
//! nothing. Each piece is copied when it is appended, so no piece is retained
//! as a span of a JS string, nothing is traced and no piece is read twice.
//!
//! WTF-8 concatenation is byte concatenation except at one seam: a piece that
//! ends in a lone high surrogate followed by one that begins with a lone low
//! surrogate form one supplementary code point. `seam` joins exactly that
//! case, so a pair split across two pieces (or across a span boundary of a
//! non-ASCII subject) comes out as the one four-byte sequence a UTF-16
//! concatenation denotes.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_memory::StorageError;
use super::perex_owner::HeapSubject;
use super::perex_runtime::{self as host, EngineError, PieceStride};
use super::perex_strings::read_error;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::{
    StringHeader, MAX_STRING_LENGTH, STRING_FLAG_HAS_LONE_SURROGATES, STRING_FLAG_WTF8_VALIDATED,
};
use crate::value::JSValue;
use perex::binding::BoundSubject;
use perex::span::{BoundSpan, ReadProgress, Span};
use perex::Budget;

#[cfg(test)]
crate::perry_thread_local! {
    /// Counts finished outputs, so a test can assert that a replacement built
    /// its result here rather than infer it from a timing.
    pub(crate) static OUTPUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// UTF-16 units of well-formed generalized WTF-8 bytes: one per lead byte of
/// a one- to three-byte sequence, two per four-byte lead, none per
/// continuation byte.
pub(super) fn wtf8_units(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0xbf => 0,
            0xf0..=0xff => 2,
            _ => 1,
        })
        .sum()
}

/// Whether WTF-8 bytes hold a surrogate code point (`ED A0..BF xx`).
pub(super) fn has_surrogate(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|w| w[0] == 0xed && (0xa0..=0xbf).contains(&w[1]))
}

/// A string the output copies spans of: a bound subject, read as bytes when
/// it is ASCII and unit by unit otherwise. The unit reader is kept, so
/// successive spans of a non-ASCII subject seek from the previous one.
pub(super) struct Text<'a, 's> {
    subject: &'a BoundSubject<HeapSubject<'s>>,
    ascii: bool,
    reader: Option<BoundSpan<'a, HeapSubject<'s>>>,
    length: usize,
}

impl<'a, 's> Text<'a, 's> {
    pub(super) fn new(subject: &'a BoundSubject<HeapSubject<'s>>) -> Result<Self, EngineError> {
        let (ascii, length) = subject
            .with_view(|input| (input.ascii_bytes().is_some(), input.len_utf16()))
            .map_err(EngineError::Subject)?;
        Ok(Self {
            subject,
            ascii,
            reader: None,
            length,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.length
    }
}

#[cfg(test)]
crate::perry_thread_local! {
    static NATIVE_RELEASE_NOTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Only owned heap capacity participates in external-side accounting.
#[inline]
fn release_native_bytes(bytes: usize) {
    if bytes != 0 {
        #[cfg(test)]
        NATIVE_RELEASE_NOTES.with(|n| n.set(n.get() + 1));
        crate::gc::gc_note_external_side_free(bytes);
    }
}

pub(super) struct Output {
    bytes: Vec<u8>,
    units: usize,
    /// A piece may have carried a surrogate code point, so `finish` derives
    /// the exact lone-surrogate flag from the bytes.
    surrogates: bool,
    noted: usize,
    stride: PieceStride,
}

impl Drop for Output {
    fn drop(&mut self) {
        release_native_bytes(self.noted);
    }
}

impl Output {
    /// An output whose buffer is sized once for `bytes`, the caller's estimate
    /// of the result (the subject's size for a replacement).
    pub(super) fn with_capacity(bytes: usize) -> Result<Self, EngineError> {
        let mut output = Self {
            bytes: Vec::new(),
            units: 0,
            surrogates: false,
            noted: 0,
            stride: PieceStride::new(),
        };
        output.reserve(bytes.min(api::OUTPUT_BYTES))?;
        Ok(output)
    }

    /// The buffer is native and untraced; the collector is told about it as
    /// external bytes, like other runtime side storage.
    fn reserve(&mut self, more: usize) -> Result<(), EngineError> {
        let total = self
            .bytes
            .len()
            .checked_add(more)
            .filter(|&n| n <= api::OUTPUT_BYTES)
            .ok_or(StorageError::Limit)?;
        if total > self.bytes.capacity() {
            self.bytes
                .try_reserve(more)
                .map_err(|_| StorageError::Allocation)?;
        }
        let capacity = self.bytes.capacity();
        if capacity > self.noted {
            let grown = capacity - self.noted;
            self.noted = capacity;
            crate::gc::gc_note_external_side_alloc(grown);
        }
        Ok(())
    }

    /// Count `units` more output units, and poll once per `POLL_UNITS` units
    /// of pieces (each piece counting one more), never per piece.
    fn count(&mut self, units: usize) -> Result<(), EngineError> {
        self.units = self
            .units
            .checked_add(units)
            .filter(|&n| n <= MAX_STRING_LENGTH)
            .ok_or(StorageError::Limit)?;
        self.stride.tick(units)
    }

    /// Join a lone high surrogate the output ends with and a lone low
    /// surrogate `next` begins with into one four-byte sequence. Returns how
    /// many bytes of `next` the join consumed (0 or 3).
    fn seam(&mut self, next: &[u8]) -> usize {
        let n = self.bytes.len();
        if n < 3 || next.len() < 3 {
            return 0;
        }
        let tail = &self.bytes[n - 3..];
        if tail[0] != 0xed || !(0xa0..=0xaf).contains(&tail[1]) {
            return 0;
        }
        if next[0] != 0xed || !(0xb0..=0xbf).contains(&next[1]) {
            return 0;
        }
        let decode = |b: &[u8]| {
            (u32::from(b[0] & 0x0f) << 12) | (u32::from(b[1] & 0x3f) << 6) | u32::from(b[2] & 0x3f)
        };
        let point = 0x10000 + ((decode(tail) - 0xd800) << 10) + (decode(next) - 0xdc00);
        self.bytes.truncate(n - 3);
        self.bytes.extend_from_slice(&[
            0xf0 | (point >> 18) as u8,
            0x80 | ((point >> 12) & 0x3f) as u8,
            0x80 | ((point >> 6) & 0x3f) as u8,
            0x80 | (point & 0x3f) as u8,
        ]);
        3
    }

    /// Append well-formed WTF-8 `bytes` (the caller counts their units).
    /// `surrogates` says they may hold a surrogate code point.
    fn wtf8(&mut self, bytes: &[u8], surrogates: bool) -> Result<(), EngineError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.reserve(bytes.len())?;
        let joined = if surrogates && self.surrogates {
            self.seam(bytes)
        } else {
            0
        };
        self.bytes.extend_from_slice(&bytes[joined..]);
        self.surrogates |= surrogates;
        Ok(())
    }

    /// Append one UTF-16 unit, joining it to a pending high surrogate.
    fn unit(&mut self, unit: u16) {
        let u = u32::from(unit);
        if u < 0x80 {
            self.bytes.push(u as u8);
            return;
        }
        if u < 0x800 {
            self.bytes
                .extend_from_slice(&[0xc0 | (u >> 6) as u8, 0x80 | (u & 0x3f) as u8]);
            return;
        }
        let encoded = [
            0xe0 | (u >> 12) as u8,
            0x80 | ((u >> 6) & 0x3f) as u8,
            0x80 | (u & 0x3f) as u8,
        ];
        if (0xd800..=0xdfff).contains(&u) {
            let joined = if self.surrogates {
                self.seam(&encoded)
            } else {
                0
            };
            self.surrogates = true;
            self.bytes.extend_from_slice(&encoded[joined..]);
        } else {
            self.bytes.extend_from_slice(&encoded);
        }
    }

    /// Append the units `start..end` of `text`.
    pub(super) fn span(
        &mut self,
        text: &mut Text<'_, '_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        if start > end || end > text.length {
            return Err(EngineError::InvalidSpan);
        }
        if start == end {
            return Ok(());
        }
        let len = end - start;
        host::charge(budget, len)?;
        if text.ascii {
            self.reserve(len)?;
            let bytes = &mut self.bytes;
            text.subject
                .with_view(|input| {
                    let source = input.ascii_bytes().ok_or(EngineError::InvalidSpan)?;
                    bytes.extend_from_slice(&source[start..end]);
                    Ok::<(), EngineError>(())
                })
                .map_err(EngineError::Subject)??;
            return self.count(len);
        }
        // At most three bytes per unit; the reservation keeps every push
        // below within capacity.
        self.reserve(len.checked_mul(3).ok_or(StorageError::Limit)?)?;
        let span = Span::new(start, end).ok_or(EngineError::InvalidSpan)?;
        let reader = match &mut text.reader {
            Some(reader) => {
                reader
                    .retarget(span)
                    .map_err(|e| read_error(e, |n| match n {}))?;
                reader
            }
            slot @ None => slot.insert(
                BoundSpan::new(text.subject, span).map_err(|e| read_error(e, |n| match n {}))?,
            ),
        };
        loop {
            let progress = reader
                .try_fold(api::QUANTUM, budget, |unit| {
                    self.unit(unit);
                    Ok::<(), EngineError>(())
                })
                .map_err(|e| read_error(e, |e| e))?;
            if progress == ReadProgress::Complete {
                break;
            }
            host::poll()?;
        }
        self.count(len)
    }

    /// Append ToString(`value`). A string is copied as it is: a validated
    /// heap string or an immediate by its bytes, any other heap string after
    /// binding proves its encoding. Anything else converts first, which may
    /// run user code.
    pub(super) fn value(
        &mut self,
        value: &RuntimeHandle<'_>,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let v = JSValue::from_bits(value.get_nanbox_u64());
        if v.is_short_string() {
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let len = v.short_string_to_buf(&mut scratch);
            let bytes = &scratch[..len];
            let units = wtf8_units(bytes);
            host::charge(budget, units)?;
            self.wtf8(bytes, has_surrogate(bytes))?;
            return self.count(units);
        }
        let scope = RuntimeHandleScope::new();
        let string = if v.is_string() {
            scope.root_string_ptr(v.as_string_ptr())
        } else {
            scope.root_string_ptr(dispatch::to_string(value)?)
        };
        self.string(&string, budget)
    }

    /// Append the whole rooted heap string `string`.
    pub(super) fn string(
        &mut self,
        string: &RuntimeHandle<'_>,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let (units, flags) = string.with_const_ptr::<StringHeader, _>(|s| unsafe {
            ((*s).utf16_len as usize, (*s).flags)
        });
        if flags & STRING_FLAG_WTF8_VALIDATED == 0 {
            // Binding validates (and on success marks) the header, so a
            // payload that is not well-formed is never copied as bytes.
            let bound = api::bind_heap_subject(*string)?;
            let validated = string.with_const_ptr::<StringHeader, _>(|s| unsafe {
                (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0
            });
            if !validated {
                let mut text = Text::new(&bound)?;
                let length = text.len();
                return self.span(&mut text, 0, length, budget);
            }
        }
        host::charge(budget, units)?;
        let surrogates = string.with_const_ptr::<StringHeader, _>(|s| unsafe {
            (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0
        });
        unsafe { string.with_string_bytes(|bytes| self.wtf8(bytes, surrogates)) }?;
        self.count(units)
    }

    /// Append ASCII bytes.
    pub(super) fn ascii(&mut self, bytes: &[u8]) -> Result<(), EngineError> {
        debug_assert!(bytes.is_ascii());
        self.wtf8(bytes, false)?;
        self.count(bytes.len())
    }

    /// Append `start..end` (bytes) of the rooted, validated string `string`,
    /// holding `units` units.
    pub(super) fn bytes_of(
        &mut self,
        string: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        units: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        host::charge(budget, units)?;
        let surrogates = string.with_const_ptr::<StringHeader, _>(|s| unsafe {
            (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0
        });
        unsafe {
            string.with_string_bytes(|bytes| {
                let piece = bytes.get(start..end).ok_or(EngineError::InvalidSpan)?;
                self.wtf8(piece, surrogates)
            })
        }?;
        self.count(units)
    }

    /// The finished string. Allocation cannot throw (a failure aborts), so
    /// no trap is armed; the buffer is dropped normally on every path.
    pub(super) fn finish(self) -> Result<f64, EngineError> {
        #[cfg(test)]
        OUTPUTS.with(|n| n.set(n.get() + 1));
        let len = self.bytes.len();
        if len == self.units {
            if let Some(short) = JSValue::try_short_string(&self.bytes) {
                return Ok(f64::from_bits(short.bits()));
            }
        }
        let limit = api::OUTPUT_BYTES.min(
            u32::MAX as usize - crate::gc::GC_HEADER_SIZE - std::mem::size_of::<StringHeader>() - 7,
        );
        if len > limit {
            return Err(StorageError::Limit.into());
        }
        let lone = self.surrogates && has_surrogate(&self.bytes);
        // Every piece was well-formed WTF-8 and every seam is joined, so the
        // payload carries the encoding proof unless a surrogate remains.
        let flags = if lone {
            STRING_FLAG_HAS_LONE_SURROGATES
        } else {
            STRING_FLAG_WTF8_VALIDATED
        };
        let string = crate::string::js_string_from_bytes_known_utf16(
            self.bytes.as_ptr(),
            len as u32,
            self.units as u32,
            flags,
        );
        Ok(crate::value::js_nanbox_string(string as i64))
    }
}

/// Offsets a replacement collects before it builds (every match's capture
/// spans, or a literal pattern's positions). Their size follows the subject
/// (matches times captures), not a fixed operation limit, so they are not
/// charged to the `MemoryBudget`: a cap there made large replacements throw
/// where Node completes (#10164). The collector is told about them as
/// external bytes, like other runtime side storage.
pub(super) struct Offsets {
    pub(super) values: smallvec::SmallVec<[u32; 16]>,
    noted: usize,
}

impl Offsets {
    pub(super) fn new() -> Self {
        Self {
            values: smallvec::SmallVec::new(),
            noted: 0,
        }
    }

    /// Make room for `more` values; every push below stays within it.
    pub(super) fn reserve(&mut self, more: usize) -> Result<(), EngineError> {
        self.values
            .try_reserve(more)
            .map_err(|_| StorageError::Allocation)?;
        let bytes = if self.values.spilled() {
            self.values.capacity() * std::mem::size_of::<u32>()
        } else {
            0
        };
        if bytes > self.noted {
            let grown = bytes - self.noted;
            self.noted = bytes;
            crate::gc::gc_note_external_side_alloc(grown);
        }
        Ok(())
    }

    pub(super) fn push(&mut self, value: usize) -> Result<(), EngineError> {
        let value = u32::try_from(value).map_err(|_| StorageError::Limit)?;
        if self.values.len() == self.values.capacity() {
            self.reserve(1)?;
        }
        self.values.push(value);
        Ok(())
    }
}

impl Drop for Offsets {
    fn drop(&mut self) {
        release_native_bytes(self.noted);
    }
}

#[cfg(test)]
mod short_offset_tests {
    use super::*;
    #[test]
    fn zero_capacity_native_storage_sends_no_release_notification() {
        let before = NATIVE_RELEASE_NOTES.with(std::cell::Cell::get);
        drop(Offsets::new());
        drop(Output::with_capacity(0).unwrap());
        assert_eq!(NATIVE_RELEASE_NOTES.with(std::cell::Cell::get), before);
        let mut offsets = Offsets::new();
        for value in 0..17 {
            offsets.push(value).unwrap();
        }
        drop(offsets);
        assert_eq!(NATIVE_RELEASE_NOTES.with(std::cell::Cell::get), before + 1);
    }

    #[test]
    fn inline_offsets_spill_without_losing_values_or_charging_stack_bytes() {
        let mut offsets = Offsets::new();
        for value in 0..16 {
            offsets.push(value).unwrap();
        }
        assert!(!offsets.values.spilled());
        assert_eq!(offsets.noted, 0);
        offsets.push(16).unwrap();
        assert!(offsets.values.spilled());
        assert!(offsets.noted >= 17 * std::mem::size_of::<u32>());
        assert_eq!(&offsets.values[..], &(0..17).collect::<Vec<u32>>()[..]);
    }
}

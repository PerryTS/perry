//! Perry's byte-preserving literal-string domain. This is not regex input:
//! malformed Buffer/FFI payloads retain their bytes and per-part metadata.
//! Search shares the UTF-16 literal path's KMP loop and offset-only scratch.
use super::perex_api as api;
use super::perex_literal_search::{each_readers, Read};
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_replace_storage::List;
use super::perex_runtime::{self as host, EngineError};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::{StringHeader, STRING_FLAG_HAS_LONE_SURROGATES};
use crate::value::js_nanbox_string;
use perex::Budget;

fn byte_len(input: &RuntimeHandle<'_>) -> usize {
    input.with_const_ptr::<StringHeader, _>(|p| unsafe { (*p).byte_len as usize })
}

struct Bytes<'a, 's> {
    root: &'a RuntimeHandle<'s>,
    since_poll: usize,
}
impl<'a, 's> Bytes<'a, 's> {
    fn new(root: &'a RuntimeHandle<'s>) -> Self {
        Self {
            root,
            since_poll: 0,
        }
    }
}
impl Read for Bytes<'_, '_> {
    fn at(&mut self, index: usize, budget: &mut Budget) -> Result<u16, EngineError> {
        host::charge(budget, 1)?;
        self.since_poll += 1;
        if self.since_poll == api::QUANTUM {
            self.since_poll = 0;
            host::poll()?;
        }
        unsafe {
            self.root.with_string_bytes(|bytes| {
                bytes
                    .get(index)
                    .copied()
                    .map(u16::from)
                    .ok_or(EngineError::InvalidSpan)
            })
        }
    }
}

/// Streaming form of Perry's compute_utf16_len_wtf8 plus bounded surrogate
/// detection. A continuation byte can contribute zero units while still
/// occupying output storage; a truncated four-byte lead contributes two.
#[derive(Default, PartialEq, Eq)]
struct Metadata {
    units: usize,
    skip: usize,
    flags: u32,
}
impl Metadata {
    fn scan(&mut self, whole: &[u8], start: usize, end: usize) {
        for i in start..end {
            let b = whole[i];
            if self.skip != 0 {
                self.skip -= 1;
            } else if b < 0x80 {
                self.units += 1;
            } else if b >= 0xc0 {
                let width = if b < 0xe0 {
                    2
                } else if b < 0xf0 {
                    3
                } else {
                    4
                };
                self.units += if width == 4 { 2 } else { 1 };
                self.skip = width - 1;
            }
            if b == 0xed
                && whole.get(i + 1).is_some_and(|b| (0xa0..=0xbf).contains(b))
                && whole.get(i + 2).is_some_and(|b| (0x80..=0xbf).contains(b))
            {
                self.flags |= STRING_FLAG_HAS_LONE_SURROGATES;
            }
        }
    }
}

/// Copy only the final JavaScript result. Every bounded source borrow ends
/// before polling/allocation, and both bases are reacquired after collection.
pub(crate) fn copy_bytes(
    input: &RuntimeHandle<'_>,
    start: usize,
    end: usize,
    budget: &mut Budget,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<*mut StringHeader, EngineError> {
    if quantum == 0 {
        return Err(EngineError::InvalidQuantum);
    }
    let count = end
        .checked_sub(start)
        .filter(|_| end <= byte_len(input))
        .ok_or(EngineError::InvalidSpan)?;
    let max_bytes = api::OUTPUT_BYTES.min(
        u32::MAX as usize - crate::gc::GC_HEADER_SIZE - std::mem::size_of::<StringHeader>() - 7,
    );
    if count > max_bytes {
        return Err(StorageError::Limit.into());
    }
    crate::string::js_string_addref(input.get_raw_mut_ptr());
    poll()?;
    let mut measured = Metadata::default();
    let mut offset = 0;
    while offset < count {
        let next = offset.saturating_add(quantum).min(count);
        host::charge(budget, next - offset)?;
        unsafe { input.with_string_bytes(|bytes| measured.scan(&bytes[start..end], offset, next)) };
        offset = next;
        if offset < count {
            poll()?;
        }
    }
    if measured.units > crate::string::MAX_STRING_LENGTH {
        return Err(StorageError::Limit.into());
    }
    poll()?;
    let scope = RuntimeHandleScope::new();
    let (output, _) = api::caught(|| crate::string::string_storage_alloc(count as u32))?;
    unsafe { crate::string::init_string_header(output, 0, 0, count as u32, 0, 0) };
    let output = scope.root_string_ptr(output);
    let mut written = Metadata::default();
    offset = 0;
    while offset < count {
        let next = offset.saturating_add(quantum).min(count);
        host::charge(budget, next - offset)?;
        output.with_mut_ptr::<StringHeader, _>(|out| unsafe {
            input.with_string_bytes(|bytes| {
                let part = &bytes[start..end];
                std::ptr::copy_nonoverlapping(
                    part.as_ptr().add(offset),
                    (crate::string::string_data(out) as *mut u8).add(offset),
                    next - offset,
                );
                written.scan(part, offset, next);
                crate::string::init_string_header(
                    out,
                    written.units as u32,
                    next as u32,
                    count as u32,
                    0,
                    written.flags,
                );
            });
        });
        offset = next;
        if offset < count {
            poll()?;
        }
    }
    if written != measured {
        return Err(EngineError::InvalidSpan);
    }
    Ok(output.get_raw_mut_ptr())
}

pub(super) fn split(
    input: &RuntimeHandle<'_>,
    needle: &RuntimeHandle<'_>,
    limit: usize,
    output: &mut List<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<(), EngineError> {
    crate::string::js_string_addref(input.get_raw_mut_ptr());
    crate::string::js_string_addref(needle.get_raw_mut_ptr());
    let (n, m) = (byte_len(input), byte_len(needle));
    if m == 0 {
        // Preserve the same bounded WTF-8-shape walk as Perry's scalar
        // split helpers. Malformed bytes still produce a part; astral
        // characters produce their two separate UTF-16 surrogate halves.
        let mut position = 0;
        let mut pending = None;
        while (position < n || pending.is_some()) && output.len() < limit {
            host::charge(budget, 1)?;
            let result = if let Some(unit) = pending.take() {
                api::caught(|| crate::string::string_from_code_unit(unit))?
            } else {
                let (advance, units, point) = unsafe {
                    input.with_string_bytes(|bytes| crate::string::wtf8_step(bytes, position))
                };
                let end = position.saturating_add(advance).min(n);
                let result = if units == 2 && point >= 0x10000 {
                    let astral = point - 0x10000;
                    pending = Some(0xdc00 + (astral & 0x3ff) as u16);
                    api::caught(|| {
                        crate::string::string_from_code_unit(0xd800 + (astral >> 10) as u16)
                    })?
                } else {
                    copy_bytes(input, position, end, budget, api::QUANTUM, &mut host::poll)?
                };
                position = end;
                result
            };
            output.push(js_nanbox_string(result as i64), budget)?;
        }
        return Ok(());
    }
    let mut source = Bytes::new(input);
    let mut left = Bytes::new(needle);
    let mut right = Bytes::new(needle);
    let mut end = 0;
    each_readers(
        (n, m),
        &mut source,
        (&mut left, &mut right),
        budget,
        memory,
        |position, budget| {
            let part = copy_bytes(input, end, position, budget, api::QUANTUM, &mut host::poll)?;
            output.push(js_nanbox_string(part as i64), budget)?;
            end = position + m;
            Ok(output.len() == limit)
        },
    )?;
    if output.len() < limit {
        let part = copy_bytes(input, end, n, budget, api::QUANTUM, &mut host::poll)?;
        output.push(js_nanbox_string(part as i64), budget)?;
    }
    Ok(())
}

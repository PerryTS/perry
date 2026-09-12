//! Materialize exact UTF-16 capture spans into final Perry string storage.
//! Input is traversed in place; no UTF-16 buffer or temporary substring exists.

use super::perex_memory::StorageError;
use super::perex_owner::{GcProgram, HeapSubject, OwnerError};
use super::perex_runtime::EngineError;
use crate::gc::RuntimeHandleScope;
use crate::string::{StringHeader, STRING_FLAG_HAS_LONE_SURROGATES};
use perex::binding::{BoundProgram, BoundSubject};
use perex::executor::ExecError;
use perex::span::{BoundSpan, ReadError, ReadProgress, Span};
use perex::Budget;
use std::mem::MaybeUninit;

pub(crate) fn read_error<E>(
    error: ReadError<OwnerError, E>,
    consumer: impl FnOnce(E) -> EngineError,
) -> EngineError {
    match error {
        ReadError::Subject(error) => EngineError::Subject(error),
        ReadError::InvalidSpan => EngineError::InvalidSpan,
        ReadError::InvalidQuantum => EngineError::InvalidQuantum,
        ReadError::ChangedPosition | ReadError::Failed => {
            EngineError::Execution(ExecError::ChangedResources)
        }
        ReadError::WorkLimit => EngineError::Execution(ExecError::WorkLimit),
        ReadError::Consumer(error) => consumer(error),
    }
}

/// Only scalar counters and at most one pending high surrogate survive a poll.
/// A pending high surrogate is emitted alone unless the next captured unit is
/// low. A pair split by the capture boundary is therefore preserved exactly.
#[derive(Default)]
pub(super) struct Encoder {
    pending: Option<u16>,
    pub(super) bytes: usize,
    pub(super) units: usize,
    pub(super) flags: u32,
}

impl Encoder {
    fn point(
        &mut self,
        point: u32,
        limit: usize,
        emit: &mut impl FnMut(&[u8]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let mut bytes = [0; 4];
        let len = if point < 0x80 {
            bytes[0] = point as u8;
            1
        } else if point < 0x800 {
            bytes[0] = 0xc0 | (point >> 6) as u8;
            bytes[1] = 0x80 | (point & 63) as u8;
            2
        } else if point < 0x10000 {
            bytes[0] = 0xe0 | (point >> 12) as u8;
            bytes[1] = 0x80 | ((point >> 6) & 63) as u8;
            bytes[2] = 0x80 | (point & 63) as u8;
            3
        } else {
            bytes[0] = 0xf0 | (point >> 18) as u8;
            bytes[1] = 0x80 | ((point >> 12) & 63) as u8;
            bytes[2] = 0x80 | ((point >> 6) & 63) as u8;
            bytes[3] = 0x80 | (point & 63) as u8;
            4
        };
        let total = self
            .bytes
            .checked_add(len)
            .filter(|&n| n <= limit)
            .ok_or(StorageError::Limit)?;
        emit(&bytes[..len])?;
        self.bytes = total;
        self.units += if point > 0xffff { 2 } else { 1 };
        if (0xd800..=0xdfff).contains(&point) {
            self.flags |= STRING_FLAG_HAS_LONE_SURROGATES;
        }
        Ok(())
    }

    pub(super) fn push(
        &mut self,
        unit: u16,
        limit: usize,
        emit: &mut impl FnMut(&[u8]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if let Some(high) = self.pending.take() {
            if (0xdc00..=0xdfff).contains(&unit) {
                let point = 0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit) - 0xdc00;
                return self.point(point, limit, emit);
            }
            self.point(u32::from(high), limit, emit)?;
        }
        if (0xd800..=0xdbff).contains(&unit) {
            self.pending = Some(unit);
            Ok(())
        } else {
            self.point(u32::from(unit), limit, emit)
        }
    }

    pub(super) fn finish(
        &mut self,
        limit: usize,
        emit: &mut impl FnMut(&[u8]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if let Some(high) = self.pending.take() {
            self.point(u32::from(high), limit, emit)?;
        }
        Ok(())
    }
}

/// Return one new heap string under the usual runtime factory contract: the
/// caller must root/store the returned pointer before its next allocation.
/// A private scope releases an unfinished output root on error or unwinding.
/// Both passes charge the same work budget. Each borrow reads at most quantum
/// units, including initial seeking, and output headers describe only the
/// initialized, valid WTF-8 prefix whenever collection is possible.
pub(crate) fn copy_span(
    subject: &BoundSubject<HeapSubject<'_>>,
    span: Span,
    budget: &mut Budget,
    max_output_bytes: usize,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<*mut StringHeader, EngineError> {
    let mut readers = [
        BoundSpan::new(subject, span).map_err(|e| read_error(e, |never| match never {}))?,
        BoundSpan::new(subject, span).map_err(|e| read_error(e, |never| match never {}))?,
    ];
    copy_units(
        Some(span.len()),
        budget,
        max_output_bytes,
        quantum,
        poll,
        |pass, quantum, budget, consume| {
            readers[pass]
                .try_fold(quantum, budget, consume)
                .map_err(|e| read_error(e, |error| error))
        },
    )
}

/// Two reusable original-string cursors for a sequence of final substrings.
/// Each pass retains its own position, so adjacent split pieces do not seek
/// repeatedly from the beginning of a non-ASCII input. Only offsets survive GC.
pub(super) struct SpanCopies<'a, 's> {
    readers: [BoundSpan<'a, HeapSubject<'s>>; 2],
}

impl<'a, 's> SpanCopies<'a, 's> {
    pub(super) fn new(subject: &'a BoundSubject<HeapSubject<'s>>) -> Result<Self, EngineError> {
        let empty = Span::new(0, 0).unwrap();
        Ok(Self {
            readers: [
                BoundSpan::new(subject, empty).map_err(|e| read_error(e, |n| match n {}))?,
                BoundSpan::new(subject, empty).map_err(|e| read_error(e, |n| match n {}))?,
            ],
        })
    }

    pub(super) fn copy(
        &mut self,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<*mut StringHeader, EngineError> {
        let span = Span::new(start, end).ok_or(EngineError::InvalidSpan)?;
        for reader in &mut self.readers {
            reader
                .retarget(span)
                .map_err(|e| read_error(e, |n| match n {}))?;
        }
        copy_units(
            Some(span.len()),
            budget,
            super::perex_api::OUTPUT_BYTES,
            super::perex_api::QUANTUM,
            &mut super::perex_runtime::poll,
            |pass, quantum, budget, consume| {
                self.readers[pass]
                    .try_fold(quantum, budget, consume)
                    .map_err(|e| read_error(e, |e| e))
            },
        )
    }
}

/// Decode a packed program name directly into its final string. No native
/// name copy or program view survives an allocation, poll or object update.
pub(crate) fn copy_name(
    program: &BoundProgram<GcProgram<'_>>,
    index: usize,
    budget: &mut Budget,
    max_output_bytes: usize,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<*mut StringHeader, EngineError> {
    let units = program
        .with_view(|p| p.named_group(index).map(|n| n.name_units().len()))
        .map_err(EngineError::Program)?
        .ok_or(EngineError::InvalidSpan)?;
    let mut offsets = [0usize; 2];
    copy_units(
        Some(units),
        budget,
        max_output_bytes,
        quantum,
        poll,
        |pass, quantum, budget, consume| {
            program
                .with_view(|p| {
                    let name = p.named_group(index).ok_or(EngineError::InvalidSpan)?;
                    for unit in name.name_units().skip(offsets[pass]).take(quantum) {
                        super::perex_runtime::charge(budget, 1)?;
                        consume(unit)?;
                        offsets[pass] += 1;
                    }
                    Ok(if offsets[pass] == units {
                        ReadProgress::Complete
                    } else {
                        ReadProgress::Pending
                    })
                })
                .map_err(EngineError::Program)?
        },
    )
}

pub(super) fn copy_units(
    unit_count: Option<usize>,
    budget: &mut Budget,
    max_output_bytes: usize,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
    mut read: impl FnMut(
        usize,
        usize,
        &mut Budget,
        &mut dyn FnMut(u16) -> Result<(), EngineError>,
    ) -> Result<ReadProgress, EngineError>,
) -> Result<*mut StringHeader, EngineError> {
    if quantum == 0 {
        return Err(EngineError::InvalidQuantum);
    }
    if unit_count.is_some_and(|n| n > crate::string::MAX_STRING_LENGTH) {
        return Err(StorageError::Limit.into());
    }
    let limit = max_output_bytes.min(
        u32::MAX as usize - crate::gc::GC_HEADER_SIZE - std::mem::size_of::<StringHeader>() - 7,
    );
    poll()?;
    let mut measured = Encoder::default();
    loop {
        match read(0, quantum, budget, &mut |unit| {
            measured.push(unit, limit, &mut |_| Ok(()))
        })? {
            ReadProgress::Pending => poll()?,
            ReadProgress::Complete => break,
        }
    }
    measured.finish(limit, &mut |_| Ok(()))?;
    if unit_count.is_some_and(|n| measured.units != n) {
        return Err(EngineError::InvalidSpan);
    }
    if measured.units > crate::string::MAX_STRING_LENGTH {
        return Err(StorageError::Limit.into());
    }
    poll()?;
    let scope = RuntimeHandleScope::new();
    let capacity = measured.bytes as u32;
    let (output, _) = crate::string::string_storage_alloc(capacity);
    // Unwritten capacity remains uninitialized. The header publishes an empty
    // prefix until complete points have been written. No GC occurs before root.
    unsafe {
        crate::string::init_string_header(output, 0, 0, capacity, 0, 0);
    }
    let output = scope.root_string_ptr(output);
    let mut encoded = Encoder::default();
    let mut offset = 0usize;
    loop {
        let progress = output.with_mut_ptr::<StringHeader, _>(|header| {
            // Reacquire BOTH bases for each bounded step. The owner getters
            // and fold perform no allocation or collection inside this scope.
            let data = unsafe {
                std::slice::from_raw_parts_mut(
                    crate::string::string_data(header) as *mut MaybeUninit<u8>,
                    capacity as usize,
                )
            };
            let mut emit = |bytes: &[u8]| {
                let end = offset.checked_add(bytes.len()).ok_or(StorageError::Limit)?;
                let target = data.get_mut(offset..end).ok_or(StorageError::Limit)?;
                for (slot, &byte) in target.iter_mut().zip(bytes) {
                    slot.write(byte);
                }
                offset = end;
                Ok(())
            };
            let progress = read(1, quantum, budget, &mut |unit| {
                encoded.push(unit, limit, &mut emit)
            });
            let progress = match progress {
                Ok(ReadProgress::Complete) => encoded
                    .finish(limit, &mut emit)
                    .map(|()| ReadProgress::Complete),
                other => other,
            };
            unsafe {
                crate::string::init_string_header(
                    header,
                    encoded.units as u32,
                    encoded.bytes as u32,
                    capacity,
                    0,
                    encoded.flags,
                );
            }
            progress
        })?;
        if progress == ReadProgress::Complete {
            break;
        }
        poll()?;
    }
    if encoded.bytes != measured.bytes
        || encoded.units != measured.units
        || encoded.flags != measured.flags
        || offset != measured.bytes
    {
        return Err(EngineError::InvalidSpan);
    }
    Ok(output.get_raw_mut_ptr::<StringHeader>())
}

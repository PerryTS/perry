//! Traced lists and output pieces for replacement. Retain original strings;
//! materialize only final JS strings. Native callback slots are mutable roots.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::subject;
use super::perex_memory::{MemoryBudget, Reservation, StorageError};
use super::perex_owner::HeapSubject;
use super::perex_runtime::{self as host, EngineError};
use super::perex_strings::{read_error, Encoder};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::js_nanbox_string;
use perex::binding::BoundSubject;
use perex::span::{BoundSpan, ReadProgress, Span};
use perex::Budget;

pub(super) fn length(s: &RuntimeHandle<'_>) -> usize {
    s.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize })
}
/// The string a handle currently holds, NaN-boxed for an entry point that roots
/// its arguments.
pub(super) fn boxed(s: &RuntimeHandle<'_>) -> f64 {
    s.with_const_ptr::<StringHeader, _>(|s| js_nanbox_string(s as i64))
}
pub(super) fn text<'a>(
    scope: &'a RuntimeHandleScope,
    value: &RuntimeHandle<'_>,
) -> Result<RuntimeHandle<'a>, EngineError> {
    Ok(scope.root_string_ptr(dispatch::to_string(value)?))
}
pub(super) struct List<'a> {
    root: RuntimeHandle<'a>,
    count: usize,
}
impl<'a> List<'a> {
    pub(super) fn new(scope: &'a RuntimeHandleScope) -> Result<Self, EngineError> {
        Ok(Self {
            root: scope.root_raw_mut_ptr(api::caught(|| crate::array::js_array_alloc(0))?),
            count: 0,
        })
    }
    pub(super) fn len(&self) -> usize {
        self.count
    }
    pub(super) fn value(&self) -> f64 {
        self.root
            .with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                crate::value::js_nanbox_pointer(array as i64)
            })
    }
    pub(super) fn get(&self, index: usize) -> f64 {
        self.root
            .with_const_ptr(|array| crate::array::js_array_get_f64(array, index as u32))
    }
    /// Append one value. The list is an ordinary GC array, so it is bounded
    /// by what can be allocated, not by a count: the scratch limit is for
    /// native buffers, and a list's length follows the subject. A replacement
    /// producing more pieces than that limit's entries once threw a memory
    /// error on subjects Node replaces in a fraction of a second (#10164).
    pub(super) fn push(&mut self, value: f64, budget: &mut Budget) -> Result<(), EngineError> {
        host::charge(budget, 1)?;
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let array = api::caught(|| {
            self.root.with_mut_ptr(|array| {
                crate::array::js_array_push_f64(array, value.get_nanbox_f64())
            })
        })?;
        self.root.set_raw_mut_ptr(array);
        self.count += 1;
        if self.count % api::QUANTUM == 0 {
            host::poll()?;
        }
        Ok(())
    }
}

pub(super) fn call(
    method: &RuntimeHandle<'_>,
    receiver: &RuntimeHandle<'_>,
    args: &List<'_>,
    memory: &MemoryBudget,
) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    if crate::proxy::js_proxy_is_proxy(method.get_nanbox_f64()) == 1 {
        let result = api::caught(|| {
            crate::proxy::js_proxy_apply(
                method.get_nanbox_f64(),
                receiver.get_nanbox_f64(),
                args.value(),
            )
        });
        crate::object::js_implicit_this_set(previous.get_nanbox_f64());
        return result;
    }
    let mut slots = Vec::<std::cell::UnsafeCell<f64>>::new();
    slots
        .try_reserve_exact(args.len())
        .map_err(|_| StorageError::Allocation)?;
    for i in 0..args.len() {
        slots.push(std::cell::UnsafeCell::new(args.get(i)));
    }
    struct Frame(u64);
    impl Drop for Frame {
        fn drop(&mut self) {
            crate::gc::js_shadow_frame_pop(self.0);
        }
    }
    let frame = Frame(crate::gc::js_shadow_frame_push(args.len() as u32));
    for (i, value) in slots.iter().enumerate() {
        crate::gc::js_shadow_slot_bind(i as u32, value.get().cast());
    }
    let reservation = Reservation::new(
        memory,
        slots.capacity().checked_mul(8).ok_or(StorageError::Limit)?,
    )?;
    let result = api::caught(|| unsafe {
        crate::object::js_implicit_this_set(receiver.get_nanbox_f64());
        crate::closure::js_native_call_value(
            method.get_nanbox_f64(),
            slots.as_ptr().cast(),
            slots.len(),
        )
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    drop(reservation);
    drop(frame);
    drop(slots);
    result
}

/// A reusable original-input reader. A read retains only Perex offsets across
/// collection, and adjacent reads do not repeat the initial Unicode seek.
pub(super) struct Units<'a, 's> {
    reader: BoundSpan<'a, HeapSubject<'s>>,
    since_poll: usize,
}
impl<'a, 's> Units<'a, 's> {
    pub(super) fn new(input: &'a BoundSubject<HeapSubject<'s>>) -> Result<Self, EngineError> {
        Ok(Self {
            reader: BoundSpan::new(input, Span::new(0, 0).unwrap())
                .map_err(|e| read_error(e, |n| match n {}))?,
            since_poll: 0,
        })
    }
    pub(super) fn at(&mut self, index: usize, budget: &mut Budget) -> Result<u16, EngineError> {
        self.reader
            .retarget(Span::new(index, index.checked_add(1).ok_or(StorageError::Limit)?).unwrap())
            .map_err(|e| read_error(e, |n| match n {}))?;
        let mut result = None;
        loop {
            let progress = self
                .reader
                .try_fold(api::QUANTUM, budget, |unit| {
                    result = Some(unit);
                    Ok::<_, EngineError>(())
                })
                .map_err(|e| read_error(e, |e| e))?;
            if progress == ReadProgress::Complete {
                break;
            }
            host::poll()?;
        }
        self.since_poll += 1;
        if self.since_poll == api::QUANTUM {
            self.since_poll = 0;
            host::poll()?;
        }
        result.ok_or(EngineError::InvalidSpan)
    }
}

/// Which string a native piece spans. A string-template replacement never
/// produces a piece from anywhere else: every piece is part of the subject or
/// part of the template, both of which outlive the replacement and are already
/// rooted by the caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Original,
    Template,
}

/// Native piece records, for the replacement paths whose pieces are all spans
/// of the subject or the template (#10411).
///
/// The JS-array backing costs about a kilobyte of traced heap per piece — three
/// `js_array_push_f64` calls, each with a handle scope and a string addref,
/// against an array the collector must trace and grow. A 550 KB subject with
/// 100,000 matches peaked at 545 MB RSS that way, against Node's 122 MB, and
/// the cost scaled with the number of pieces rather than the size of the data.
/// These records are 12 bytes each, allocated once, and traced by nobody.
struct NativePieces {
    records: Vec<(Source, u32, u32)>,
    noted: usize,
}
impl NativePieces {
    /// Keep the operation's external-byte accounting in step with the vector's
    /// capacity, as `Spans` does for the span list.
    fn note_growth(&mut self) -> Result<(), EngineError> {
        let bytes = self.records.capacity() * std::mem::size_of::<(Source, u32, u32)>();
        if bytes > self.noted {
            let grown = bytes - self.noted;
            self.noted = bytes;
            api::caught(|| crate::gc::gc_note_external_side_alloc(grown))?;
        }
        Ok(())
    }
}
impl Drop for NativePieces {
    fn drop(&mut self) {
        crate::gc::gc_note_external_side_free(self.noted);
    }
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Counts `Pieces` that kept their records native, so a test can assert
    /// which backing a replacement took rather than infer it from a timing.
    pub(crate) static NATIVE_PIECES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) struct Pieces<'a> {
    list: List<'a>,
    /// `Some` while every piece is a span of the subject or the template. A
    /// callback replacement produces JS strings from user code, so it keeps
    /// the list.
    native: Option<NativePieces>,
    units: usize,
}
impl<'a> Pieces<'a> {
    pub(super) fn new(scope: &'a RuntimeHandleScope) -> Result<Self, EngineError> {
        Ok(Self {
            list: List::new(scope)?,
            native: None,
            units: 0,
        })
    }

    /// A `Pieces` whose records stay native. ONLY a string-template
    /// replacement may use it, and the two backings must never both be
    /// populated: `walk` emits every native record before any list entry, so a
    /// caller that mixed them would silently lose the interleaving and produce
    /// reordered output. `append` and `whole` therefore refuse a native
    /// `Pieces` rather than falling back to the list, and `walk` asserts the
    /// same invariant.
    pub(super) fn new_native(scope: &'a RuntimeHandleScope) -> Result<Self, EngineError> {
        #[cfg(test)]
        NATIVE_PIECES.with(|n| n.set(n.get() + 1));
        Ok(Self {
            list: List::new(scope)?,
            native: Some(NativePieces {
                records: Vec::new(),
                noted: 0,
            }),
            units: 0,
        })
    }

    /// Record a span of the subject, natively when this `Pieces` is native.
    pub(super) fn append_original(
        &mut self,
        original: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        self.append_tagged(Source::Original, original, start, end, budget)
    }

    /// Record a span of the template, natively when this `Pieces` is native.
    pub(super) fn append_template(
        &mut self,
        template: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        self.append_tagged(Source::Template, template, start, end, budget)
    }

    fn append_tagged(
        &mut self,
        source: Source,
        handle: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        if self.native.is_none() {
            return self.append(handle, start, end, budget);
        }
        if start > end || end > length(handle) {
            return Err(EngineError::InvalidSpan);
        }
        if start == end {
            return Ok(());
        }
        self.units = self
            .units
            .checked_add(end - start)
            .filter(|&n| n <= crate::string::MAX_STRING_LENGTH)
            .ok_or(StorageError::Limit)?;
        host::charge(budget, 1)?;
        let native = self.native.as_mut().expect("checked above");
        native.records.push((source, start as u32, end as u32));
        native.note_growth()
    }
    pub(super) fn append(
        &mut self,
        source: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        // A native `Pieces` must not also hold list entries: `walk` emits all
        // of one before any of the other, so mixing them reorders the output
        // rather than merely costing the saving. Fail here, where the mistake
        // is, instead of producing wrong bytes at `finish`.
        debug_assert!(
            self.native.is_none(),
            "a native Pieces cannot take an arbitrary source; walk would reorder the output"
        );
        if self.native.is_some() {
            return Err(EngineError::InvalidSpan);
        }
        if start > end || end > length(source) {
            return Err(EngineError::InvalidSpan);
        }
        if start == end {
            return Ok(());
        }
        self.units = self
            .units
            .checked_add(end - start)
            .filter(|&n| n <= crate::string::MAX_STRING_LENGTH)
            .ok_or(StorageError::Limit)?;
        self.list.push(boxed(source), budget)?;
        self.list.push(start as f64, budget)?;
        self.list.push(end as f64, budget)
    }
    pub(super) fn whole(
        &mut self,
        source: &RuntimeHandle<'_>,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        self.append(source, 0, length(source), budget)
    }
    fn walk(
        &self,
        original: &RuntimeHandle<'_>,
        template: Option<&RuntimeHandle<'_>>,
        budget: &mut Budget,
        mut step: impl FnMut(
            &mut BoundSpan<'_, HeapSubject<'_>>,
            &mut Budget,
        ) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let original_subject = subject(*original)?;
        let mut original_reader = BoundSpan::new(&original_subject, Span::new(0, 0).unwrap())
            .map_err(|e| read_error(e, |n| match n {}))?;
        let template_subject = template.map(|t| subject(*t)).transpose()?;
        let mut template_reader = template_subject
            .as_ref()
            .map(|t| {
                BoundSpan::new(t, Span::new(0, 0).unwrap())
                    .map_err(|e| read_error(e, |n| match n {}))
            })
            .transpose()?;
        if let Some(native) = self.native.as_ref() {
            for &(source, start, end) in &native.records {
                let span =
                    Span::new(start as usize, end as usize).ok_or(EngineError::InvalidSpan)?;
                match source {
                    Source::Original => {
                        original_reader
                            .retarget(span)
                            .map_err(|e| read_error(e, |n| match n {}))?;
                        step(&mut original_reader, budget)?;
                    }
                    Source::Template => {
                        let reader = template_reader.as_mut().ok_or(EngineError::InvalidSpan)?;
                        reader
                            .retarget(span)
                            .map_err(|e| read_error(e, |n| match n {}))?;
                        step(reader, budget)?;
                    }
                }
            }
            debug_assert!(
                self.list.len() == 0,
                "native and list records must never both be populated; walk emits \
                 every native record before any list entry"
            );
            if self.list.len() != 0 {
                return Err(EngineError::InvalidSpan);
            }
            return Ok(());
        }
        for index in (0..self.list.len()).step_by(3) {
            let local = RuntimeHandleScope::new();
            let source = local.root_string_ptr(crate::value::js_get_string_pointer_unified(
                self.list.get(index),
            ) as *const StringHeader);
            let span = Span::new(
                self.list.get(index + 1) as usize,
                self.list.get(index + 2) as usize,
            )
            .ok_or(EngineError::InvalidSpan)?;
            let same = |a: &RuntimeHandle<'_>, b: &RuntimeHandle<'_>| {
                a.with_const_ptr::<StringHeader, _>(|a| b.with_const_ptr(|b| a == b))
            };
            if same(&source, original) {
                original_reader
                    .retarget(span)
                    .map_err(|e| read_error(e, |n| match n {}))?;
                step(&mut original_reader, budget)?;
            } else if template.is_some_and(|t| same(t, &source)) {
                let reader = template_reader.as_mut().unwrap();
                reader
                    .retarget(span)
                    .map_err(|e| read_error(e, |n| match n {}))?;
                step(reader, budget)?;
            } else {
                let source = subject(source)?;
                let mut reader =
                    BoundSpan::new(&source, span).map_err(|e| read_error(e, |n| match n {}))?;
                step(&mut reader, budget)?;
            }
        }
        Ok(())
    }
    pub(super) fn finish(
        &self,
        original: &RuntimeHandle<'_>,
        template: Option<&RuntimeHandle<'_>>,
        budget: &mut Budget,
    ) -> Result<*mut StringHeader, EngineError> {
        let limit = api::OUTPUT_BYTES.min(
            u32::MAX as usize - crate::gc::GC_HEADER_SIZE - std::mem::size_of::<StringHeader>() - 7,
        );
        let mut measured = Encoder::default();
        self.walk(original, template, budget, |reader, budget| loop {
            let p = reader
                .try_fold(api::QUANTUM, budget, |u| {
                    measured.push(u, limit, &mut |_| Ok(()))
                })
                .map_err(|e| read_error(e, |e| e))?;
            host::poll()?;
            if p == ReadProgress::Complete {
                return Ok(());
            }
        })?;
        measured.finish(limit, &mut |_| Ok(()))?;
        if measured.units != self.units {
            return Err(EngineError::InvalidSpan);
        }
        let scope = RuntimeHandleScope::new();
        let output = api::caught(|| {
            let (p, _) = crate::string::string_storage_alloc(measured.bytes as u32);
            unsafe {
                crate::string::init_string_header(p, 0, 0, measured.bytes as u32, 0, 0);
            }
            p
        })?;
        let output = scope.root_string_ptr(output);
        let mut encoded = Encoder::default();
        let mut written = 0usize;
        self.walk(original, template, budget, |reader, budget| loop {
            let p = output.with_mut_ptr::<StringHeader, _>(|header| {
                let mut emit = |bytes: &[u8]| {
                    let end = written
                        .checked_add(bytes.len())
                        .filter(|&n| n <= measured.bytes)
                        .ok_or(StorageError::Limit)?;
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            crate::string::string_data(header).cast_mut().add(written),
                            bytes.len(),
                        );
                    }
                    written = end;
                    Ok(())
                };
                let p = reader
                    .try_fold(api::QUANTUM, budget, |u| encoded.push(u, limit, &mut emit))
                    .map_err(|e| read_error(e, |e| e));
                unsafe {
                    crate::string::init_string_header(
                        header,
                        encoded.units as u32,
                        encoded.bytes as u32,
                        measured.bytes as u32,
                        0,
                        encoded.flags,
                    );
                }
                p
            })?;
            host::poll()?;
            if p == ReadProgress::Complete {
                return Ok(());
            }
        })?;
        output.with_mut_ptr::<StringHeader, _>(|header| {
            encoded.finish(limit, &mut |bytes| {
                let end = written
                    .checked_add(bytes.len())
                    .filter(|&n| n <= measured.bytes)
                    .ok_or(StorageError::Limit)?;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        crate::string::string_data(header).cast_mut().add(written),
                        bytes.len(),
                    );
                }
                written = end;
                Ok(())
            })?;
            unsafe {
                crate::string::init_string_header(
                    header,
                    encoded.units as u32,
                    encoded.bytes as u32,
                    measured.bytes as u32,
                    0,
                    encoded.flags,
                );
            }
            Ok::<_, EngineError>(())
        })?;
        if (encoded.units, encoded.bytes, encoded.flags, written)
            != (
                measured.units,
                measured.bytes,
                measured.flags,
                measured.bytes,
            )
        {
            return Err(EngineError::InvalidSpan);
        }
        Ok(output.with_mut_ptr(|output| output))
    }
}

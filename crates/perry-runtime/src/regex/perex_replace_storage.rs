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
    pub(super) fn push(&mut self, value: f64, budget: &mut Budget) -> Result<(), EngineError> {
        host::charge(budget, 1)?;
        if self.count >= api::SCRATCH_BYTES / 8 {
            return Err(StorageError::Limit.into());
        }
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

pub(super) struct Pieces<'a> {
    list: List<'a>,
    units: usize,
}
impl<'a> Pieces<'a> {
    pub(super) fn new(scope: &'a RuntimeHandleScope) -> Result<Self, EngineError> {
        Ok(Self {
            list: List::new(scope)?,
            units: 0,
        })
    }
    pub(super) fn append(
        &mut self,
        source: &RuntimeHandle<'_>,
        start: usize,
        end: usize,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
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

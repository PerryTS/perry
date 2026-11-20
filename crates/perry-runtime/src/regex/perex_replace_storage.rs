//! Traced lists and replacer calls for replacement and split. Native callback
//! slots are mutable roots; the output itself is built by `perex_output`.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_memory::{MemoryBudget, Reservation, StorageError};
use super::perex_owner::HeapSubject;
use super::perex_runtime::{self as host, EngineError};
use super::perex_strings::read_error;
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

    /// `push` for a list no code outside this operation has seen yet, such as
    /// a `split` result still being built. Nothing can have frozen, sealed or
    /// wrapped it, so an append that fits its capacity stores in place and
    /// cannot throw; it skips the catch frame `push` sets up, which was about
    /// a tenth of a short `split`. An append that must grow takes `push`.
    pub(super) fn push_unseen(
        &mut self,
        value: f64,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let fits = self
            .root
            .with_const_ptr::<crate::array::ArrayHeader, _>(|array| unsafe {
                (*array).length < (*array).capacity
            });
        if !fits {
            return self.push(value, budget);
        }
        host::charge(budget, 1)?;
        let array = self
            .root
            .with_mut_ptr(|array| crate::array::js_array_push_f64(array, value));
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
    if crate::proxy_hooks::proxy_is_proxy(method.get_nanbox_f64()) == 1 {
        return api::caught(|| {
            crate::proxy_hooks::proxy_apply(
                method.get_nanbox_f64(),
                receiver.get_nanbox_f64(),
                args.value(),
            )
        });
    }
    let mut slots = Vec::<std::cell::UnsafeCell<f64>>::new();
    slots
        .try_reserve_exact(args.len())
        .map_err(|_| StorageError::Allocation)?;
    for i in 0..args.len() {
        slots.push(std::cell::UnsafeCell::new(args.get(i)));
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    for cell in &slots {
        unsafe {
            scope.root_nanbox_cell(cell);
        }
    }
    let reservation = Reservation::new(
        memory,
        slots.capacity().checked_mul(8).ok_or(StorageError::Limit)?,
    )?;
    let result = api::caught(|| unsafe {
        crate::closure::native_call_value_this(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
            slots.as_ptr().cast(),
            slots.len(),
        )
    });
    drop(reservation);
    drop(scope);
    drop(slots);
    result
}

/// Arguments for a replacer call, produced straight into shadow-stack slots.
///
/// `call` builds its slots by copying a JS array the caller filled one push at
/// a time. For a replacer invoked once per match that array is pure overhead:
/// it is allocated, grown as each argument is pushed, read back out, and
/// dropped, and no user code can observe it. Here the slots are bound to the
/// shadow stack *before* any argument is produced, so a value is a traced root
/// from the moment it is written and producing the next one may allocate and
/// collect freely. The buffer is sized once and outlives the match loop.
///
/// Not usable for a proxy replacer: `js_proxy_apply` takes the arguments as a
/// JS array, which the `apply` trap observes, so that path keeps `List`.
pub(super) struct NativeArgs {
    slots: Vec<std::cell::UnsafeCell<f64>>,
}

impl NativeArgs {
    pub(super) fn new(count: usize) -> Result<Self, EngineError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| StorageError::Allocation)?;
        for _ in 0..count {
            slots.push(std::cell::UnsafeCell::new(f64::from_bits(
                crate::value::TAG_UNDEFINED,
            )));
        }
        Ok(Self { slots })
    }
}

/// `call` for an ordinary replacer, with the arguments produced by `fill`
/// directly into `args`.
///
/// Every slot is reset to `undefined` and bound before `fill` runs, so an
/// argument `fill` does not write stays `undefined` (an unset capture), and one
/// it does write is rooted immediately. `fill` may therefore allocate between
/// arguments, which is what producing a match's capture strings does.
pub(super) fn call_native(
    method: &RuntimeHandle<'_>,
    receiver: &RuntimeHandle<'_>,
    args: &mut NativeArgs,
    memory: &MemoryBudget,
    fill: impl FnOnce(&mut dyn FnMut(usize, f64)) -> Result<(), EngineError>,
) -> Result<f64, EngineError> {
    let slots = &args.slots;
    for slot in slots {
        unsafe { *slot.get() = f64::from_bits(crate::value::TAG_UNDEFINED) };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    for cell in slots {
        unsafe {
            scope.root_nanbox_cell(cell);
        }
    }
    let mut set = |i: usize, value: f64| {
        unsafe {
            *slots[i].get() = value;
        }
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    };
    fill(&mut set)?;
    let reservation = Reservation::new(
        memory,
        slots.len().checked_mul(8).ok_or(StorageError::Limit)?,
    )?;
    let result = api::caught(|| unsafe {
        crate::closure::native_call_value_this(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
            slots.as_ptr().cast(),
            slots.len(),
        )
    });
    drop(reservation);
    drop(scope);
    result
}

/// How many units of output pieces a replacement or split appends between GC
/// safepoint polls, each piece counting one more unit.
///
/// Polling per piece ran the whole budgeted trigger ladder (~436
/// instructions, no cheap "nothing is due" precheck) for a handful of units
/// each. Polling far less often costs peak RSS where the pieces come with
/// garbage (a replacer's strings): at `api::QUANTUM` (4096) it was +13.2%
/// median on an allocating replace at n=1,000,000, over the accepted +10%
/// budget. This value is therefore a measured trade: small enough to keep the
/// collector's openings, large enough that a piece of two or three units no
/// longer buys a poll of its own.
pub(super) const POLL_UNITS: usize = 512;

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

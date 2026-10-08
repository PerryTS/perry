//! The runtime's stream state record (decision 91 follow-up to #12104 G1/G2).
//!
//! Every runtime stream (Readable, Writable, Duplex, Transform, PassThrough,
//! their subclasses and the native-payload stream families) keeps its
//! private state in one fixed record of NaN-boxed words (a runtime-owned
//! array no JS code can reach), found from the object itself: a plain
//! stream's `meta.native_state` names the record; a stream family's
//! `meta.native_state` names its payload cell, whose one record word names
//! it. A slot is a fixed index, so reading or writing state is a few
//! dependent loads plus the array's write barrier: no key interning, no
//! own-key scan, no shape transition, and nothing of it is an own property
//! of the stream. The record is an ordinary young arena object, so a
//! short-lived stream's state dies in the minor that finds the stream dead.
//!
//! What node shows (`_readableState`, `_writableState`, `destroyed`,
//! `readableEnded`, …) stays ordinary properties; only the runtime's private
//! bookkeeping lives here. An absent slot reads `undefined`, exactly as the
//! old absent hidden key did, so every reader keeps its meaning.
//!
//! Both edges (the meta record's `native_state`, the cell's record word) are
//! traced and rewritten by their GC descriptors (`gc/layout_slot_visit.rs`);
//! the record is reachable only through its stream. There is no side table,
//! address map or registry.

use super::*;
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

macro_rules! stream_slots {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        /// One word of a stream's state record.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u16)]
        pub(crate) enum Slot {
            $($(#[$doc])* $name,)*
        }

        /// Words in every stream record.
        pub(crate) const STREAM_RECORD_SLOT_COUNT: usize = [$(Slot::$name),*].len();

        /// Slot names, for the `enumerable_state` sabotage only.
        #[cfg(test)]
        const SLOT_NAMES: &[&str] = &[$(stringify!($name)),*];
    };
}

stream_slots! {
    /// A stream family's payload cell (`native_payload`), POINTER_TAG-boxed:
    /// the stream's `native_state` names this record, and the record the cell.
    PayloadCell,
    /// The object's native-this alias word (`object::native_this_alias`, its
    /// traced record encoding) when the stream is also an aliased native
    /// construction: one `native_state` word, one owner.
    NativeAlias,
    ReadableFlag,
    WritableFlag,
    /// A Transform (direct, subclass, PassThrough, or a native-payload family).
    TransformFlag,
    ReadableChunks,
    ReadableSourceIterator,
    ReadableError,
    ReadableSignal,
    ReadableRead,
    ReadableReadInvoked,
    ReadableDefaultReadError,
    /// #1539: bytes currently buffered (`push()`'s highWaterMark answer).
    ReadableBuffered,
    ReadableHwm,
    ReadablePending,
    ReadableResumeScheduled,
    ReadableBase64Remainder,
    /// #9490: the incomplete trailing UTF-8 sequence between `push()` calls.
    ReadableUtf8Remainder,
    ReadableFromPromisePending,
    DrainScheduled,
    ReadableScheduled,
    EndScheduled,
    EndEmitted,
    Ended,
    /// An emitter's `captureRejections` flag (node's `this[kCapture]`).
    CaptureRejections,
    Disturbed,
    Pipes,
    PipeNoEnd,
    PipeEndPending,
    AutoDestroy,
    EmitClose,
    PipelineCallbackDone,
    ReadableLivePush,
    CloseEmitted,
    Construct,
    Destroy,
    WritableWrite,
    WritableWritev,
    FinishScheduled,
    FinishEmitted,
    WritableCorked,
    WritableBuffered,
    WritableLength,
    WritableNeedDrain,
    WritableObjectMode,
    WritableDecodeStrings,
    WritableDefaultEncoding,
    WritablePendingFinishCallback,
    WritableFinal,
    WritableFinalInvoked,
    WritableFinalPending,
    WritableWriting,
    WritableSync,
    WritableBufferProcessing,
    WritableCustomSink,
    DuplexPairPeer,
    TransformCallback,
    TransformFlush,
    TransformPassThrough,
    TransformFinishing,
    /// `end()` ran on a Transform with writes still in flight (node's `ending`).
    TransformEndPending,
    /// A Transform's held write completion (node's `kCallback`).
    TransformHeldCallback,
    ComposePriming,
    ComposePendingError,
    /// A LazyTransform's constructor options, until its first stream use.
    NativeStreamOptions,
    NativeOp,
    NativeChunk,
    NativeLen,
    NativeCb,
    NativeConsumed,
    NativeFlushKind,
    /// The record's input is consumed but its completion is held because the
    /// readable side is full (node's Transform `kCallback`).
    NativeDone,
    NativeParked,
    NativeScheduled,
    NativeRunning,
}

/// `stream`'s record, when it has one. Never allocates.
#[inline]
fn record_of(stream: f64) -> Option<*mut ArrayHeader> {
    let obj = object_ptr_from_value(stream)?;
    // SAFETY: a live ordinary object (checked above).
    let meta = unsafe { (*obj).meta };
    if meta.is_null() {
        return None;
    }
    // SAFETY: the meta record of a live object.
    record_in_word(unsafe { (*meta).native_state })
}

/// The record a `native_state` word names, when it names one.
#[inline]
fn record_in_word(word: u64) -> Option<*mut ArrayHeader> {
    is_stream_record_word(word).then(|| (word & crate::value::POINTER_MASK) as *mut ArrayHeader)
}

/// Is `word` (an `ObjectMeta.native_state`) the fixed-size stream record?
/// The shorter native-this alias record uses the same traced array encoding.
#[inline]
pub(crate) fn is_stream_record_word(word: u64) -> bool {
    crate::native_payload::is_payload_state_word(word)
        // SAFETY: a pointer-tagged `native_state` word is a traced edge to a
        // live GC cell.
        && unsafe { gc_type_for_ptr((word & crate::value::POINTER_MASK) as usize) }
            == Some(crate::gc::GC_TYPE_ARRAY)
        && unsafe { (*((word & crate::value::POINTER_MASK) as *const ArrayHeader)).length as usize } == STREAM_RECORD_SLOT_COUNT
}

/// The payload cell word a stream family's record holds (0 for a plain
/// stream's record). `word` is a stream record word.
#[inline]
pub(crate) fn record_payload_cell(word: u64) -> u64 {
    let record = (word & crate::value::POINTER_MASK) as *const ArrayHeader;
    // SAFETY: a live record of the fixed length.
    if !is_stream_record_word(word) {
        return 0;
    }
    let bits = unsafe { *crate::array::array_elements_ptr(record).add(Slot::PayloadCell as usize) };
    if bits == crate::value::TAG_UNDEFINED {
        0
    } else {
        bits
    }
}

/// The native-this alias word the record `native_state` names carries, or 0
/// (no record, or no alias).
#[inline]
pub(crate) fn record_alias_word(native_state: u64) -> u64 {
    let Some(record) = record_in_word(native_state) else {
        return 0;
    };
    // SAFETY: a live record of the fixed length.
    let bits = unsafe { *crate::array::array_elements_ptr(record).add(Slot::NativeAlias as usize) };
    if bits == crate::value::TAG_UNDEFINED {
        0
    } else {
        bits
    }
}

/// Store the traced native-this alias record in the stream record. The array
/// store supplies the barrier for this child edge.
pub(crate) fn store_record_alias_word(native_state: u64, alias: u64) -> bool {
    let Some(record) = record_in_word(native_state) else {
        return false;
    };
    store(record, Slot::NativeAlias, f64::from_bits(alias));
    true
}

/// Attach `cell_word` to the record `native_state` names, if it names one:
/// a payload attached after the stream constructor ran (`attach_to_object`)
/// joins the record instead of displacing it.
pub(crate) fn store_record_payload_cell(native_state: u64, cell_word: u64) -> bool {
    let Some(record) = record_in_word(native_state) else {
        return false;
    };
    store(record, Slot::PayloadCell, f64::from_bits(cell_word));
    true
}

/// The value in `slot`, `None` when absent (`undefined`) or when `stream`
/// has no record.
#[inline]
pub(crate) fn read_slot(stream: f64, slot: Slot) -> Option<f64> {
    let record = record_of(stream)?;
    // SAFETY: `slot` indexes the record's fixed span.
    let bits = unsafe { *crate::array::array_elements_ptr(record).add(slot as usize) };
    (bits != crate::value::TAG_UNDEFINED).then(|| f64::from_bits(bits))
}

#[inline]
fn store(record: *mut ArrayHeader, slot: Slot, value: f64) {
    #[cfg(test)]
    if super::native_hooks::stream_sabotage("record_barrier") {
        // SAFETY: as below, minus the barrier.
        unsafe { *crate::array::array_elements_ptr(record).add(slot as usize) = value.to_bits() };
        return;
    }
    // The array store: string aliasing, layout note and write barrier.
    crate::array::js_array_set_f64(record, slot as u32, value);
}

/// Store `value` in `slot`, giving `stream` a record first if it has none.
/// An object whose `native_state` already belongs to another family (a
/// non-stream payload, a timer id, …) cannot carry stream state; the store
/// is dropped there, as a store to a non-object always was.
pub(crate) fn write_slot(stream: f64, slot: Slot, value: f64) {
    #[cfg(test)]
    if super::native_hooks::stream_sabotage("enumerable_state") {
        enumerable_state_sabotage(stream, slot, value);
    }
    if let Some(record) = record_of(stream) {
        store(record, slot, value);
        return;
    }
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        // Absent already.
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let value = scope.root_nanbox_f64(value);
    ensure_record(stream.get_nanbox_f64());
    if let Some(record) = record_of(stream.get_nanbox_f64()) {
        store(record, slot, value.get_nanbox_f64());
    }
}

/// Give a stream being constructed its record: a fixed array of
/// [`STREAM_RECORD_SLOT_COUNT`] words, every one absent, held in the stream's
/// own `native_state`. A stream family's payload cell, attached before the
/// stream constructor runs, moves into the record's [`Slot::PayloadCell`];
/// a native-this alias (`http.ServerResponse.call(this, req)` on an object
/// the runtime also keeps stream state for) moves into
/// [`Slot::NativeAlias`]. An object whose `native_state` belongs to another
/// kind (a weak collection's storage, a packed scalar) gets none.
pub(crate) fn ensure_record(stream: f64) -> bool {
    if record_of(stream).is_some() {
        return true;
    }
    let Some(obj) = object_ptr_from_value(stream) else {
        return false;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let meta =
        obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe { crate::object::object_meta_ensure(o) });
    if meta.is_null() {
        return false;
    }
    let word = unsafe { (*meta).native_state };
    let alias = crate::object::native_this_alias::is_alias_word(word);
    #[cfg(test)]
    if alias && super::native_hooks::stream_sabotage("record_displaces_alias") {
        return false;
    }
    if word != 0 && !alias && crate::native_payload::payload_cell_of_word(word).is_none() {
        return false;
    }
    let previous = scope.root_nanbox_f64(f64::from_bits(word));
    let record = crate::array::js_array_alloc_with_length_exact(STREAM_RECORD_SLOT_COUNT as u32);
    // SAFETY: a fresh array of exactly that length. The cell word (a malloc
    // cell, never moved) and `undefined` need no barrier on a newborn array.
    unsafe {
        let elements = crate::array::array_elements_ptr(record);
        for i in 0..STREAM_RECORD_SLOT_COUNT {
            // GC_STORE_AUDIT(POINTER_FREE): the absent marker.
            elements.add(i).write(crate::value::TAG_UNDEFINED);
        }
    }
    if alias {
        store(record, Slot::NativeAlias, previous.get_nanbox_f64());
    } else if word != 0 {
        // The cell does not move; its word is still current.
        store(record, Slot::PayloadCell, f64::from_bits(word));
    }
    let record_word = crate::value::JSValue::pointer(record as *const u8).bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe {
        let meta = (*o).meta;
        (*meta).native_state = record_word;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            record_word,
        );
    });
    true
}

/// `enumerable_state` sabotage: the pre-record shape, where runtime state was
/// an enumerable own property of the stream.
#[cfg(test)]
fn enumerable_state_sabotage(stream: f64, slot: Slot, value: f64) {
    let Some(obj) = object_ptr_from_value(stream) else {
        return;
    };
    let name = SLOT_NAMES[slot as usize];
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(
        obj.get_raw_mut_ptr::<ObjectHeader>(),
        key,
        value.get_nanbox_f64(),
    );
}

/// Test seam: every word of `stream`'s record, raw.
#[cfg(test)]
pub(crate) fn test_record_slot_bits(stream: f64) -> Option<Vec<u64>> {
    let record = record_of(stream)?;
    let elements = unsafe { crate::array::array_elements_ptr(record) };
    Some(
        (0..STREAM_RECORD_SLOT_COUNT)
            .map(|i| unsafe { *elements.add(i) })
            .collect(),
    )
}

/// Test seams: a slot no runtime path reads while a stream is idle.
#[cfg(test)]
pub(crate) fn test_write_inert_slot(stream: f64, value: f64) {
    write_slot(stream, Slot::ComposePendingError, value);
}

#[cfg(test)]
pub(crate) fn test_read_inert_slot(stream: f64) -> f64 {
    read_slot(stream, Slot::ComposePendingError)
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
}

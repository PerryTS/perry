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

        /// Private words in every stream record.
        const SLOT_COUNT: usize = [$(Slot::$name),*].len();

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
    Destroyed,
    Closed,
    ReadableAborted,
    ReadableFlowing,
    Readable,
    ReadableEnded,
    ReadableEncoding,
    Writable,
    WritableEnded,
    WritableFinished,
    ReadableFlag,
    WritableFlag,
    /// A Transform (direct, subclass, PassThrough, or a native-payload family).
    TransformFlag,
    ReadableSourceIterator,
    ReadableError,
    ReadableSignal,
    ReadableRead,
    ReadableReadInvoked,
    /// node's `kSync`: the stream's `_read` is running.
    ReadableSync,
    /// node's `kReadingMore`: a `maybeReadMore` tick is queued.
    ReadableReadingMore,
    /// A flow tick for chunks a synchronous `_read` buffered is queued.
    ReadableFlowScheduled,
    ReadableDefaultReadError,
    ReadableObjectMode,
    /// HTTP response JS state, traced with the payload and native alias.
    HttpResponseState,
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
    PipeNoEnd,
    PipeEndPending,
    AutoDestroy,
    EmitClose,
    PipelineCallbackDone,
    ReadableLivePush,
    CloseEmitted,
    Construct,
    ConstructEvent,
    ConstructPending,
    Destroy,
    WritableWrite,
    WritableWritev,
    FinishScheduled,
    FinishEmitted,
    WritableBuffered,
    WritableNeedDrain,
    WritableObjectMode,
    WritableDecodeStrings,
    WritableDefaultEncoding,
    WritablePendingFinishCallback,
    WritableFinal,
    WritableFinalInvoked,
    WritableFinalPending,
    WritableOption,
    WritableWriteCallback,
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
    let obj = crate::object::field_get_set::runtime_read_site::object_receiver(stream)?;
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
    store(record, Slot::NativeAlias as usize, f64::from_bits(alias));
    true
}

/// Attach `cell_word` to the record `native_state` names, if it names one:
/// a payload attached after the stream constructor ran (`attach_to_object`)
/// joins the record instead of displacing it.
pub(crate) fn store_record_payload_cell(native_state: u64, cell_word: u64) -> bool {
    let Some(record) = record_in_word(native_state) else {
        return false;
    };
    store(
        record,
        Slot::PayloadCell as usize,
        f64::from_bits(cell_word),
    );
    true
}

/// A stream's public state datum: an own data property of its
/// `_readableState` / `_writableState` object, whose birth shape fixes its
/// slot. The record keeps one word per datum, after the private slots, used
/// only until that side's state object is published (the constructor's
/// normalization stores precede it) and for a stream that has none. A
/// distinct type from [`Slot`], so a
/// record read ([`read_slot`]) can never name a public datum and never
/// reaches an ordinary property Get: the record getters stay GC leaves by
/// construction, and only the readers of a public datum carry the Get's
/// effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub(crate) enum StateData {
    ReadableHwm,
    ReadableChunks,
    ReadableBuffered,
    Pipes,
    WritableHwm,
    WritableLength,
    WritableCorked,
    WritableWriteLen,
}

/// Words in every stream record: the private slots, then one word per
/// [`StateData`] (its value until the state object is published).
pub(crate) const STREAM_RECORD_SLOT_COUNT: usize = SLOT_COUNT + STATE_DATA_COUNT;
const STATE_DATA_COUNT: usize = STATE_DATA.len();
const STATE_DATA: [StateData; 8] = [
    StateData::ReadableHwm,
    StateData::ReadableChunks,
    StateData::ReadableBuffered,
    StateData::Pipes,
    StateData::WritableHwm,
    StateData::WritableLength,
    StateData::WritableCorked,
    StateData::WritableWriteLen,
];

impl StateData {
    /// The record word that holds the datum before publication.
    #[inline]
    const fn record_index(self) -> usize {
        SLOT_COUNT + self as usize
    }

    #[inline]
    pub(crate) const fn readable(self) -> bool {
        matches!(
            self,
            StateData::ReadableHwm
                | StateData::ReadableChunks
                | StateData::ReadableBuffered
                | StateData::Pipes
        )
    }

    /// The datum's property name on its state object.
    fn field(self) -> crate::runtime_state_key::NamedStateKey {
        match self {
            StateData::ReadableHwm | StateData::WritableHwm => {
                crate::runtime_state_key!(b"highWaterMark")
            }
            StateData::ReadableChunks => crate::runtime_state_key!(b"buffer"),
            StateData::ReadableBuffered | StateData::WritableLength => {
                crate::runtime_state_key!(b"length")
            }
            StateData::Pipes => crate::runtime_state_key!(b"pipes"),
            StateData::WritableCorked => crate::runtime_state_key!(b"corked"),
            StateData::WritableWriteLen => crate::runtime_state_key!(b"writelen"),
        }
    }
}

/// `stream`'s own `_readableState` / `_writableState` object, read from its
/// layout.
fn state_data_object(stream: f64, readable: bool) -> Option<*mut ObjectHeader> {
    #[inline(always)]
    fn own(
        object: *mut ObjectHeader,
        side: crate::runtime_state_key::NamedStateKey,
    ) -> Option<f64> {
        unsafe { side.read_own(object) }
    }
    let object = crate::object::field_get_set::runtime_read_site::object_receiver(stream)?;
    // A stream born on its instance layout holds the state at a fixed slot.
    // SAFETY: a live ordinary object (checked above).
    if let Some(slot) =
        unsafe { super::constructors::instance_birth::state_object_slot(object, readable) }
    {
        // SAFETY: the layout's slots are inline storage of the live object.
        let bits = unsafe {
            *((object as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
                .add(slot as usize)
        };
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        return crate::object::field_get_set::runtime_read_site::object_receiver(f64::from_bits(
            bits,
        ));
    }
    let value = if readable {
        own(object, crate::runtime_state_key!(b"_readableState"))
    } else {
        own(object, crate::runtime_state_key!(b"_writableState"))
    }?;
    crate::object::field_get_set::runtime_read_site::object_receiver(value)
}

/// The value in `slot`, `None` when absent (`undefined`) or when `stream`
/// has no record. A direct load behind the record check: no property read,
/// no call that can collect or reenter.
#[inline(never)]
pub(crate) fn read_slot(stream: f64, slot: Slot) -> Option<f64> {
    record_word(stream, slot as usize)
}

#[inline]
fn record_word(stream: f64, index: usize) -> Option<f64> {
    let record = record_of(stream)?;
    // SAFETY: `index` is a Slot or StateData word of the record's fixed span.
    let bits = unsafe { *crate::array::array_elements_ptr(record).add(index) };
    (bits != crate::value::TAG_UNDEFINED).then(|| f64::from_bits(bits))
}

/// The public datum `data`, `None` when it is `undefined`. On the state's
/// birth shape it is one slot load; any other layout (a key added, deleted
/// or redefined) takes the ordinary Get node performs.
#[inline(never)]
pub(crate) fn read_state_data(stream: f64, data: StateData) -> Option<f64> {
    let Some(state) = state_data_object(stream, data.readable()) else {
        return record_word(stream, data.record_index());
    };
    let value = super::state_view::read_birth_data(state, data)
        .unwrap_or_else(|| state_data_get(state, data));
    (value.to_bits() != crate::value::TAG_UNDEFINED).then_some(value)
}

/// The ordinary `Get(state, name)` for a state off its birth shape: an own
/// accessor or an inherited value runs as in node.
#[cold]
#[inline(never)]
fn state_data_get(state: *mut ObjectHeader, data: StateData) -> f64 {
    // SAFETY: `state` was checked by `object_receiver`, nothing ran since.
    unsafe { data.field().read_object(state) }
}

/// Store the public datum `data`: an ordinary Set on the published state
/// object, else its record word.
pub(crate) fn write_state_data(stream: f64, data: StateData, value: f64) {
    if let Some(state) = state_data_object(stream, data.readable()) {
        data.field().write_value(box_pointer(state.cast()), value);
        return;
    }
    write_record(stream, data.record_index(), value);
}

#[inline(never)]
fn store(record: *mut ArrayHeader, slot: usize, value: f64) {
    #[cfg(test)]
    if super::native_hooks::stream_sabotage("record_barrier") {
        // SAFETY: as below, minus the barrier.
        unsafe { *crate::array::array_elements_ptr(record).add(slot) = value.to_bits() };
        return;
    }
    // The array store: string aliasing, layout note and write barrier.
    crate::string::js_string_addref_if_heap_string(value);
    // SAFETY: record_of/ensure_record proved this caller-owned plain array,
    // and Slot indexes its exact fixed span. Keep the canonical slot store,
    // numeric-layout note, string aliasing and complete write barrier. The
    // traced edge was reloaded below the last collection; no safepoint occurs
    // between record_of/ensure_record and the live header flag read.
    unsafe {
        let flags = (*crate::gc::header_from_trusted_user_ptr(record.cast()))._reserved;
        crate::array::store_array_slot_resolved(record, slot, value, flags);
    };
}

/// Store `value` in `slot`, giving `stream` a record first if it has none.
/// An object whose `native_state` already belongs to another family (a
/// non-stream payload, a timer id, …) cannot carry stream state; the store
/// is dropped there, as a store to a non-object always was.
pub(crate) fn write_slot(stream: f64, slot: Slot, value: f64) {
    write_record(stream, slot as usize, value);
}

fn write_record(stream: f64, slot: usize, value: f64) {
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

/// Constructor normalization fills one already-owned record through its
/// slots, without re-resolving the receiver for every field.
pub(crate) fn initialize_slots(stream: f64, slots: &[(Slot, f64)], data: &[(StateData, f64)]) {
    let Some(record) = record_of(stream) else {
        return;
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let words = slots
        .iter()
        .map(|&(slot, value)| (slot as usize, value))
        .chain(
            data.iter()
                .map(|&(data, value)| (data.record_index(), value)),
        );
    for (slot, value) in words {
        #[cfg(test)]
        if super::native_hooks::stream_sabotage("enumerable_state") {
            enumerable_state_sabotage(stream, slot, value);
        }
        #[cfg(test)]
        if super::native_hooks::stream_sabotage("record_barrier") {
            store(record, slot, value);
            continue;
        }
        crate::string::js_string_addref_if_heap_string(value);
        // The canonical bulk-fill protocol keeps the layout note and every
        // born-old remembered edge. The receiver's owned record is resolved
        // once, and the scope prevents any safepoint across this fill.
        unsafe {
            crate::array::note_array_slot_layout_only(record, slot, value.to_bits());
            // The record may already be live with a prefix, and the
            // layout-only helper does no EK work: a hole leaves the prefix.
            if value.to_bits() == crate::value::TAG_HOLE {
                crate::array::ek_lower_prefix(record, slot as u32);
            }
        }
    }
}

/// The normalized value of `data` the constructor stored before publishing
/// the state object.
pub(crate) fn constructor_data(stream: f64, data: StateData) -> Option<f64> {
    record_word(stream, data.record_index())
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
        // A1_UNPROVEN: object_ptr_from_value(stream): a JS value, no type test (kind-A/B question for S0b-S0e).
        obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe { crate::object::object_meta_ensure(crate::object::MetaObj::runtime_born(o), crate::object::MetaReason::RuntimeHandle) });
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
        store(
            record,
            Slot::NativeAlias as usize,
            previous.get_nanbox_f64(),
        );
    } else if word != 0 {
        // The cell does not move; its word is still current.
        store(record, Slot::PayloadCell as usize, f64::from_bits(word));
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
fn enumerable_state_sabotage(stream: f64, slot: usize, value: f64) {
    let Some(obj) = object_ptr_from_value(stream) else {
        return;
    };
    let data_name;
    let name = match SLOT_NAMES.get(slot) {
        Some(name) => *name,
        None => {
            data_name = format!("{:?}", STATE_DATA[slot - SLOT_COUNT]);
            data_name.as_str()
        }
    };
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

/// Binding access to HTTP response state through the existing traced record.
#[no_mangle]
pub extern "C" fn js_node_http_response_state_get(owner: f64) -> f64 {
    read_slot(owner, Slot::HttpResponseState).unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}
#[no_mangle]
pub extern "C" fn js_node_http_response_state_set(owner: f64, state: f64) {
    write_slot(owner, Slot::HttpResponseState, state);
}

//! Optional emitter/stream probes for core object and Array paths.
//!
//! The existing native events installer fills these slots. With no possible
//! stream instances the probes return their ordinary non-stream result.
use crate::feature_hooks::Hook;

static JS_EVENT_EMITTER_SUBCLASS_INIT: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_event_emitter_subclass_init(this: f64, options: f64) -> f64 {
    JS_EVENT_EMITTER_SUBCLASS_INIT
        .get()
        .map_or_else(|| this, |f| f(this, options))
}

static JS_EVENT_EMITTER_ASYNC_RESOURCE_SUBCLASS_INIT: Hook<extern "C" fn(f64, f64) -> f64> =
    Hook::empty();
pub(crate) fn js_event_emitter_async_resource_subclass_init(this: f64, options: f64) -> f64 {
    JS_EVENT_EMITTER_ASYNC_RESOURCE_SUBCLASS_INIT
        .get()
        .map_or_else(|| this, |f| f(this, options))
}

// Shape and key-slot proofs belong to the object model's layout image.
pub(crate) use crate::object::native_instance_layout::{derived_layout, instance_layout_slot};

static IS_CLASSIC_STREAM_INSTANCE_VALUE: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_classic_stream_instance_value(value: f64) -> bool {
    IS_CLASSIC_STREAM_INSTANCE_VALUE
        .get()
        .map_or_else(|| false, |f| f(value))
}

static IS_CLASSIC_STREAM_INSTANCE_OF: Hook<fn(f64, &str) -> bool> = Hook::empty();
pub(crate) fn is_classic_stream_instance_of(value: f64, name: &str) -> bool {
    IS_CLASSIC_STREAM_INSTANCE_OF
        .get()
        .map_or_else(|| false, |f| f(value, name))
}

static READABLE_HANDLE_ASYNC_ITERATOR: Hook<fn(f64) -> Option<f64>> = Hook::empty();
pub(crate) fn readable_handle_async_iterator(value: f64) -> Option<f64> {
    READABLE_HANDLE_ASYNC_ITERATOR
        .get()
        .map_or_else(|| None, |f| f(value))
}

static IS_STREAM_RECORD_WORD: Hook<fn(u64) -> bool> = Hook::empty();
pub(crate) fn is_stream_record_word(word: u64) -> bool {
    IS_STREAM_RECORD_WORD
        .get()
        .map_or_else(|| false, |f| f(word))
}

static RECORD_PAYLOAD_CELL: Hook<fn(u64) -> u64> = Hook::empty();
pub(crate) fn record_payload_cell(word: u64) -> u64 {
    RECORD_PAYLOAD_CELL.get().map_or_else(|| 0, |f| f(word))
}

static STORE_RECORD_PAYLOAD_CELL: Hook<fn(u64, u64) -> bool> = Hook::empty();
pub(crate) fn store_record_payload_cell(word: u64, value: u64) -> bool {
    STORE_RECORD_PAYLOAD_CELL
        .get()
        .map_or_else(|| false, |f| f(word, value))
}

static RECORD_ALIAS_WORD: Hook<fn(u64) -> u64> = Hook::empty();
pub(crate) fn record_alias_word(word: u64) -> u64 {
    RECORD_ALIAS_WORD.get().map_or_else(|| 0, |f| f(word))
}

static STORE_RECORD_ALIAS_WORD: Hook<fn(u64, u64) -> bool> = Hook::empty();
pub(crate) fn store_record_alias_word(word: u64, value: u64) -> bool {
    STORE_RECORD_ALIAS_WORD
        .get()
        .map_or_else(|| false, |f| f(word, value))
}

static NOTE_BIRTH_CARRIERS: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn note_birth_carriers() -> () {
    NOTE_BIRTH_CARRIERS.get().map_or_else(|| (), |f| f())
}

static ENSURE_LAZY_STREAM: Hook<fn(f64) -> f64> = Hook::empty();
pub(crate) fn ensure_lazy_stream(value: f64) -> f64 {
    ENSURE_LAZY_STREAM.get().map_or_else(|| value, |f| f(value))
}

/// Emitter construction owns only emitter initialization. Stream probes are
/// installed by stream births, whose layouts make those probes meaningful.
pub(crate) fn install_emitter() {
    JS_EVENT_EMITTER_SUBCLASS_INIT.set(crate::node_stream::js_event_emitter_subclass_init);
    JS_EVENT_EMITTER_ASYNC_RESOURCE_SUBCLASS_INIT
        .set(crate::node_stream::js_event_emitter_async_resource_subclass_init);
}

pub(crate) fn install() {
    install_emitter();
    IS_CLASSIC_STREAM_INSTANCE_VALUE.set(crate::node_stream::is_classic_stream_instance_value);
    IS_CLASSIC_STREAM_INSTANCE_OF.set(crate::node_stream::is_classic_stream_instance_of);
    READABLE_HANDLE_ASYNC_ITERATOR
        .set(crate::node_stream::async_iterator::readable_handle_async_iterator);
    IS_STREAM_RECORD_WORD.set(crate::node_stream::is_stream_record_word);
    RECORD_PAYLOAD_CELL.set(crate::node_stream::record_payload_cell);
    STORE_RECORD_PAYLOAD_CELL.set(crate::node_stream::store_record_payload_cell);
    RECORD_ALIAS_WORD.set(crate::node_stream::record_alias_word);
    STORE_RECORD_ALIAS_WORD.set(crate::node_stream::store_record_alias_word);
    NOTE_BIRTH_CARRIERS.set(crate::node_stream::note_birth_carriers);
    ENSURE_LAZY_STREAM.set(crate::node_stream::ensure_lazy_stream);
}

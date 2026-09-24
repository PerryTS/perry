//! node:stream — live `_readableState` / `_writableState` views (#11197).
//!
//! Node gives every Readable a `ReadableState` and every Writable a
//! `WritableState`, and real stream code reads and writes them directly:
//! undici's `BodyReadable` runs `this._readableState.dataEmitted = false` in
//! its constructor and reads `endEmitted` / `closeEmitted` / `ended` /
//! `destroyed` / `errored` / `encoding` / `length` / `buffer` as live state.
//!
//! Perry keeps stream state in hidden fields on the stream object itself, so
//! the state object here is a VIEW, not a copy: one small object per stream
//! side, carrying only a back-pointer to its stream (under an internal key
//! that is non-enumerable), whose prototype is a per-thread
//! `ReadableState` / `WritableState` prototype holding one accessor pair per
//! field. Every getter reads the stream's own hidden state at call time, so
//! nothing can drift out of sync with `push()` / `read()` / `end()` /
//! `destroy()`. This mirrors Node's own layout, where almost all of these
//! fields are accessors on `ReadableState.prototype` over a bit field.
//!
//! Setters: `dataEmitted` writes through to the stream (it is a plain flag
//! the runtime also sets when it emits `'data'`). Every other field accepts
//! and ignores a write, so a library poking at state (common in stream
//! helpers) can neither throw in strict mode nor desynchronize the stream.
//!
//! GC: the two prototypes are cached per thread and reported to the collector
//! through a registered mutable root scanner (they are runtime-side caches of
//! heap pointers). Each view reaches its stream through an ordinary object
//! field, so it is traced like any other edge.
use super::*;
use crate::object::{AccessorDescriptor, ObjectHeader, PropertyAttrs};
use std::cell::{Cell, RefCell};

/// Internal back-pointer from a state view to its stream. Installed
/// non-enumerable, so `Object.keys` / `for…in` / `util.inspect` skip it, and
/// the stream JSON hook serializes a view as Node's state shape instead of
/// following it.
pub(super) const STREAM_STATE_OWNER_KEY: &[u8] = b"__perry_stream_state_owner";
const STREAM_DATA_EMITTED_KEY: &[u8] = b"__perryStreamDataEmitted";
const STREAM_CLOSE_EMITTED_KEY: &[u8] = b"__perryStreamCloseEmitted";

const READABLE_KIND: usize = 0;
const WRITABLE_KIND: usize = 1;

/// `ReadableState` fields, in Node's prototype order where it has them.
const READABLE_FIELDS: &[&str] = &[
    "objectMode",
    "highWaterMark",
    "buffer",
    "bufferIndex",
    "length",
    "pipes",
    "pipesCount",
    "flowing",
    "ended",
    "endEmitted",
    "readableListening",
    "resumeScheduled",
    "errorEmitted",
    "emitClose",
    "autoDestroy",
    "destroyed",
    "closed",
    "closeEmitted",
    "dataEmitted",
    "errored",
    "encoding",
];

/// `WritableState` fields.
const WRITABLE_FIELDS: &[&str] = &[
    "objectMode",
    "highWaterMark",
    "length",
    "corked",
    "finalCalled",
    "needDrain",
    "ending",
    "ended",
    "finished",
    "destroyed",
    "decodeStrings",
    "errorEmitted",
    "emitClose",
    "autoDestroy",
    "closed",
    "closeEmitted",
    "errored",
    "defaultEncoding",
];

thread_local! {
    /// NaN-boxed `ReadableState` / `WritableState` prototypes (0 = not built).
    static STATE_PROTOS: RefCell<[u64; 2]> = const { RefCell::new([0, 0]) };
    static STATE_PROTO_SCANNER_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

fn state_proto_root_scanner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    STATE_PROTOS.with(|protos| {
        for slot in protos.borrow_mut().iter_mut() {
            if *slot != 0 {
                visitor.visit_nanbox_u64_slot(slot);
            }
        }
    });
}

fn ensure_state_proto_scanner() {
    STATE_PROTO_SCANNER_REGISTERED.with(|registered| {
        if !registered.get() {
            crate::gc::gc_register_mutable_root_scanner_named(
                "node_stream_state_protos",
                state_proto_root_scanner,
            );
            registered.set(true);
        }
    });
}

fn bool_bits(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

/// The stream a state view belongs to, from the accessor's receiver.
fn view_owner() -> Option<f64> {
    let this = crate::object::js_implicit_this_get();
    get_hidden_value(this, hidden_key(STREAM_STATE_OWNER_KEY))
}

fn hidden_array_or_empty(stream: f64, key: *mut crate::string::StringHeader) -> f64 {
    match get_hidden_value(stream, key) {
        Some(value) if is_array_like_value(value) => value,
        _ => box_pointer(crate::array::js_array_alloc(0) as *const u8),
    }
}

fn array_len_value(value: f64) -> f64 {
    if !is_array_like_value(value) {
        return 0.0;
    }
    crate::array::js_array_length(raw_ptr_from_value(value) as *const crate::array::ArrayHeader)
        as f64
}

fn readable_field(stream: f64, field: &str) -> f64 {
    match field {
        "objectMode" => bool_bits(readable_object_mode(stream)),
        "highWaterMark" => {
            get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false))
        }
        "buffer" => hidden_array_or_empty(stream, hidden_chunks_key()),
        "bufferIndex" => 0.0,
        "length" => get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0),
        "pipes" => hidden_array_or_empty(stream, hidden_stream_pipes_key()),
        "pipesCount" => array_len_value(hidden_array_or_empty(stream, hidden_stream_pipes_key())),
        "flowing" => readable_flowing_value(stream),
        // `ended` is "EOF was pushed"; `readableEnded` is the readable side's
        // own flag (a Duplex's writable `end()` must not set it).
        "ended" => bool_bits(has_truthy_hidden(stream, hidden_key(b"readableEnded"))),
        "endEmitted" => bool_bits(has_truthy_hidden(stream, hidden_end_emitted_key())),
        "readableListening" => {
            bool_bits(stream_listener_count_for_event(stream, string_value(b"readable")) > 0)
        }
        "resumeScheduled" => bool_bits(has_truthy_hidden(
            stream,
            hidden_readable_resume_scheduled_key(),
        )),
        _ => common_field(stream, field),
    }
}

fn writable_field(stream: f64, field: &str) -> f64 {
    match field {
        "objectMode" => bool_bits(has_truthy_hidden(stream, hidden_writable_object_mode_key())),
        "highWaterMark" => get_hidden_value(stream, hidden_key(b"writableHighWaterMark"))
            .unwrap_or_else(|| default_hwm(false)),
        "length" => writable_length(stream),
        "corked" => writable_corked_count(stream),
        "finalCalled" => bool_bits(has_truthy_hidden(
            stream,
            hidden_writable_final_invoked_key(),
        )),
        "needDrain" => bool_bits(writable_need_drain(stream)),
        "ending" | "ended" => bool_bits(has_truthy_hidden(stream, hidden_key(b"writableEnded"))),
        "finished" => bool_bits(has_truthy_hidden(stream, hidden_finish_emitted_key())),
        "decodeStrings" => get_hidden_value(stream, hidden_writable_decode_strings_key())
            .unwrap_or_else(|| bool_bits(true)),
        "defaultEncoding" => get_hidden_value(stream, hidden_writable_default_encoding_key())
            .unwrap_or_else(|| string_value(b"utf8")),
        _ => common_field(stream, field),
    }
}

/// Fields both sides share: they live once on the stream in Perry, exactly as
/// Node keeps them in sync across a Duplex's two states.
fn common_field(stream: f64, field: &str) -> f64 {
    match field {
        "destroyed" => bool_bits(stream_destroyed(stream)),
        "closed" => bool_bits(has_truthy_hidden(stream, hidden_key(b"closed"))),
        "closeEmitted" => bool_bits(has_truthy_hidden(
            stream,
            hidden_key(STREAM_CLOSE_EMITTED_KEY),
        )),
        "errored" => readable_hidden_error(stream).unwrap_or(f64::from_bits(TAG_NULL)),
        "errorEmitted" => bool_bits(readable_hidden_error(stream).is_some()),
        "emitClose" => bool_bits(stream_emit_close_enabled(stream)),
        "autoDestroy" => bool_bits(stream_auto_destroy_enabled(stream)),
        "dataEmitted" => bool_bits(has_truthy_hidden(
            stream,
            hidden_key(STREAM_DATA_EMITTED_KEY),
        )),
        "encoding" => readable_encoding_value(stream),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

fn field_name(id: usize) -> Option<(usize, &'static str)> {
    let kind = id / 64;
    let fields = match kind {
        READABLE_KIND => READABLE_FIELDS,
        WRITABLE_KIND => WRITABLE_FIELDS,
        _ => return None,
    };
    fields.get(id % 64).map(|name| (kind, *name))
}

fn captured_field(closure: *const ClosureHeader) -> Option<(usize, &'static str)> {
    if closure.is_null() {
        return None;
    }
    let id = js_closure_get_capture_f64(closure, 0);
    if !(0.0..128.0).contains(&id) {
        return None;
    }
    field_name(id as usize)
}

extern "C" fn stream_state_get(closure: *const ClosureHeader) -> f64 {
    let Some((kind, field)) = captured_field(closure) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let Some(stream) = view_owner() else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    if kind == READABLE_KIND {
        readable_field(stream.get_nanbox_f64(), field)
    } else {
        writable_field(stream.get_nanbox_f64(), field)
    }
}

extern "C" fn stream_state_set(closure: *const ClosureHeader, value: f64) -> f64 {
    let Some((_, field)) = captured_field(closure) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if field == "dataEmitted" {
        if let Some(stream) = view_owner() {
            let scope = crate::gc::RuntimeHandleScope::new();
            let stream = scope.root_nanbox_f64(stream);
            let flag = bool_bits(crate::value::js_is_truthy(value) != 0);
            let key = hidden_key(STREAM_DATA_EMITTED_KEY);
            set_hidden_value(stream.get_nanbox_f64(), key, flag);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Build (once per thread) the prototype for `kind`, returning it NaN-boxed.
fn state_proto(kind: usize) -> f64 {
    let cached = STATE_PROTOS.with(|protos| protos.borrow()[kind]);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    ensure_state_proto_scanner();
    crate::closure::js_register_closure_arity(stream_state_get as *const u8, 0);
    crate::closure::js_register_closure_arity(stream_state_set as *const u8, 1);
    let fields = if kind == READABLE_KIND {
        READABLE_FIELDS
    } else {
        WRITABLE_FIELDS
    };
    let proto_obj = crate::object::js_object_alloc(0, fields.len() as u32);
    let proto_bits = crate::value::js_nanbox_pointer(proto_obj as i64).to_bits();
    // Publish before the accessor installs allocate: from here on the cache
    // slot is the root, and every use below re-reads it.
    STATE_PROTOS.with(|protos| protos.borrow_mut()[kind] = proto_bits);
    let current_proto = || {
        let bits = STATE_PROTOS.with(|protos| protos.borrow()[kind]);
        (bits & crate::value::POINTER_MASK) as *mut ObjectHeader
    };
    for (index, name) in fields.iter().enumerate() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let id = (kind * 64 + index) as f64;
        let getter = scope.root_raw_mut_ptr(js_closure_alloc(stream_state_get as *const u8, 1));
        getter.with_mut_ptr(|g| js_closure_set_capture_f64(g, 0, id));
        let setter = scope.root_raw_mut_ptr(js_closure_alloc(stream_state_set as *const u8, 1));
        setter.with_mut_ptr(|s| js_closure_set_capture_f64(s, 0, id));
        let key = scope.root_raw_mut_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ));
        unsafe {
            key.with_mut_ptr(|key: *mut crate::StringHeader| {
                crate::object::ensure_key_in_keys_array(current_proto(), key)
            });
        }
        let get_bits = getter.with_mut_ptr(|g: *mut ClosureHeader| {
            crate::value::js_nanbox_pointer(g as i64).to_bits()
        });
        let set_bits = setter.with_mut_ptr(|s: *mut ClosureHeader| {
            crate::value::js_nanbox_pointer(s as i64).to_bits()
        });
        // Non-enumerable + configurable, like Node's `ObjectDefineProperties`
        // over `ReadableState.prototype`. Nothing allocates from the reads
        // above to this install.
        crate::object::install_fresh_accessor_property(
            current_proto() as usize,
            (*name).to_string(),
            AccessorDescriptor {
                get: get_bits,
                set: set_bits,
            },
            PropertyAttrs::new(true, false, true),
        );
    }
    f64::from_bits(STATE_PROTOS.with(|protos| protos.borrow()[kind]))
}

/// Attach a fresh `_readableState` / `_writableState` view to `stream`.
fn install_state_view(stream: f64, kind: usize, property: &[u8]) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let proto = scope.root_nanbox_f64(state_proto(kind));
    let view = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 1) as *const u8
    ));
    let owner_key = hidden_key(STREAM_STATE_OWNER_KEY);
    set_hidden_value(view.get_nanbox_f64(), owner_key, stream.get_nanbox_f64());
    // Keyed by the view's address in the descriptor side table; nothing
    // allocates between this read and the install.
    crate::object::set_builtin_property_attrs(
        raw_ptr_from_value(view.get_nanbox_f64()),
        String::from_utf8_lossy(STREAM_STATE_OWNER_KEY).into_owned(),
        PropertyAttrs::new(true, false, true),
    );
    crate::object::prototype_chain::object_set_user_prototype(
        raw_ptr_from_value(view.get_nanbox_f64()),
        proto.get_nanbox_f64().to_bits(),
    );
    let property_key = hidden_key(property);
    set_hidden_value(stream.get_nanbox_f64(), property_key, view.get_nanbox_f64());
}

pub(super) fn install_readable_state_view(stream: f64) {
    install_state_view(stream, READABLE_KIND, b"_readableState");
}

pub(super) fn install_writable_state_view(stream: f64) {
    install_state_view(stream, WRITABLE_KIND, b"_writableState");
}

/// Node sets `state.dataEmitted = true` whenever a `'data'` event goes out.
pub(super) fn note_data_emitted(stream: f64) {
    let key = hidden_key(STREAM_DATA_EMITTED_KEY);
    if !has_truthy_hidden(stream, key) {
        set_hidden_value(stream, hidden_key(STREAM_DATA_EMITTED_KEY), bool_bits(true));
    }
}

/// Node sets `closeEmitted` right before `'close'` would be emitted, whether
/// or not `emitClose` lets the event itself out.
pub(super) fn note_close_emitted(stream: f64) {
    set_hidden_value(
        stream,
        hidden_key(STREAM_CLOSE_EMITTED_KEY),
        bool_bits(true),
    );
}

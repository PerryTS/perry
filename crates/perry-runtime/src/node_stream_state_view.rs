//! node:stream — live `_readableState` / `_writableState` views (#11197).
//!
//! Node gives every Readable a `ReadableState` and every Writable a
//! `WritableState`, and real stream code reads and writes them directly:
//! undici's `BodyReadable` runs `this._readableState.dataEmitted = false` in
//! its constructor and reads `endEmitted` / `closeEmitted` / `ended` /
//! `destroyed` / `errored` / `encoding` / `length` / `buffer` as live state.
//!
//! Public data fields are own properties on the state's validated birth
//! shape. Private flags remain in the stream's existing traced state record.
//! A state's traced native_state edge names its stream; no hidden string key
//! affects reflection or JSON. Prototype accessors project the private flags.
use super::*;
use crate::object::{AccessorDescriptor, ObjectHeader, PropertyAttrs};
use std::cell::{Cell, RefCell};

pub(super) const STREAM_CLOSE_EMITTED_KEY: Slot = Slot::CloseEmitted;

const READABLE_KIND: usize = 0;
const WRITABLE_KIND: usize = 1;

/// `ReadableState` fields, in Node's prototype order where it has them.
const READABLE_FIELDS: &[&str] = &[
    "objectMode",
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
    "writable",
];

crate::perry_thread_local! {
    /// NaN-boxed `ReadableState` / `WritableState` prototypes (0 = not built).
    // Keep stream TLS resolver records separate from unrelated merged globals.
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_streams")]
    static STATE_PROTOS: RefCell<[u64; 2]> = const { RefCell::new([0, 0]) };
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_streams")]
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
fn view_owner(this: crate::closure::JsThis) -> Option<f64> {
    let this = this.as_f64();
    let obj = object_ptr_from_value(this)?;
    // A1_UNPROVEN: record-first: stream object layout word (kind-A/B question for S0f).
    let meta = unsafe { crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)) };
    if meta.is_null() {
        return None;
    }
    let owner = unsafe { (*meta).native_state };
    crate::native_payload::is_payload_state_word(owner).then_some(f64::from_bits(owner))
}

fn hidden_array_or_empty(stream: f64, key: impl StateKey) -> f64 {
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
        "ended" => bool_bits(has_truthy_hidden(stream, Slot::ReadableEnded)),
        "endEmitted" => bool_bits(has_truthy_hidden(stream, hidden_end_emitted_key())),
        "readableListening" => bool_bits(
            stream_listener_count_for_event(stream, literal_string_value(b"readable")) > 0,
        ),
        "resumeScheduled" => bool_bits(has_truthy_hidden(
            stream,
            hidden_readable_resume_scheduled_key(),
        )),
        _ => common_field(stream, field),
    }
}

fn writable_field(stream: f64, field: &str) -> f64 {
    match field {
        "writable" => super::state_record::read_slot(stream, Slot::WritableOption)
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
        "objectMode" => bool_bits(has_truthy_hidden(stream, hidden_writable_object_mode_key())),
        "highWaterMark" => {
            get_hidden_value(stream, StateData::WritableHwm).unwrap_or_else(|| default_hwm(false))
        }
        "length" => writable_length(stream),
        "corked" => writable_corked_count(stream),
        "finalCalled" => bool_bits(has_truthy_hidden(
            stream,
            hidden_writable_final_invoked_key(),
        )),
        "needDrain" => bool_bits(writable_need_drain(stream)),
        "ending" | "ended" => bool_bits(has_truthy_hidden(stream, Slot::WritableEnded)),
        "finished" => bool_bits(has_truthy_hidden(stream, hidden_finish_emitted_key())),
        "decodeStrings" => get_hidden_value(stream, hidden_writable_decode_strings_key())
            .unwrap_or_else(|| bool_bits(true)),
        "defaultEncoding" => get_hidden_value(stream, hidden_writable_default_encoding_key())
            .unwrap_or_else(|| literal_string_value(b"utf8")),
        _ => common_field(stream, field),
    }
}

/// Fields both sides share: they live once on the stream in Perry, exactly as
/// Node keeps them in sync across a Duplex's two states.
fn common_field(stream: f64, field: &str) -> f64 {
    match field {
        "destroyed" => bool_bits(stream_destroyed(stream)),
        "closed" => bool_bits(has_truthy_hidden(stream, Slot::Closed)),
        "closeEmitted" => bool_bits(has_truthy_hidden(stream, STREAM_CLOSE_EMITTED_KEY)),
        "errored" => readable_hidden_error(stream).unwrap_or(f64::from_bits(TAG_NULL)),
        "errorEmitted" => bool_bits(readable_hidden_error(stream).is_some()),
        "emitClose" => bool_bits(stream_emit_close_enabled(stream)),
        "autoDestroy" => bool_bits(stream_auto_destroy_enabled(stream)),
        // One flag with `readableDidRead` / `isDisturbed()`, as in Node.
        "dataEmitted" => bool_bits(has_truthy_hidden(stream, hidden_disturbed_key())),
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

extern "C" fn stream_state_get(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let Some((kind, field)) = captured_field(closure) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let Some(stream) = view_owner(this) else {
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

extern "C" fn stream_state_set(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let Some((_, field)) = captured_field(closure) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if field == "dataEmitted" {
        if let Some(stream) = view_owner(this) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let stream = scope.root_nanbox_f64(stream);
            let emitted = crate::value::js_is_truthy(value) != 0;
            let key = hidden_disturbed_key();
            set_hidden_value(stream.get_nanbox_f64(), key, bool_bits(emitted));
            set_visible_readable_did_read(stream.get_nanbox_f64(), emitted);
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
    let fields = if kind == READABLE_KIND {
        READABLE_FIELDS
    } else {
        WRITABLE_FIELDS
    };
    let proto_desc = if kind == READABLE_KIND {
        std::ptr::addr_of!(crate::object::runtime_layouts::READABLE_STATE_PROTO)
    } else {
        std::ptr::addr_of!(crate::object::runtime_layouts::WRITABLE_STATE_PROTO)
    };
    let proto_obj = unsafe { crate::object::layout_desc::perry_alloc_layout(proto_desc) };
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
        let getter = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(stream_state_get, 0; with_declared(0)),
            1,
        ));
        getter.with_mut_ptr(|g| js_closure_set_capture_f64(g, 0, id));
        let setter = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(stream_state_set, 1; with_declared(1)),
            1,
        ));
        setter.with_mut_ptr(|s| js_closure_set_capture_f64(s, 0, id));
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
    let proto = f64::from_bits(STATE_PROTOS.with(|protos| protos.borrow()[kind]));
    // Built: the READABLE_STATE / WRITABLE_STATE layouts name this prototype.
    // It keeps its declared layout, so it is named, not sealed.
    let serial = if kind == READABLE_KIND {
        crate::closure::shape::INTRINSIC_SERIAL_READABLE_STATE
    } else {
        crate::closure::shape::INTRINSIC_SERIAL_WRITABLE_STATE
    };
    unsafe {
        crate::object::builtin_proto::name_runtime_prototype(
            crate::value::js_nanbox_get_pointer(proto) as *mut crate::object::ObjectHeader,
            serial,
        );
    }
    proto
}

/// Internal reads use the very shape that birthed the public state. A changed
/// descriptor, deletion, key addition or replacement stops this proof; the
/// caller then performs the same ordinary Get through its existing read site.
/// Each side reads its declared layout id directly (a GC leaf: no call).
#[inline]
pub(super) fn read_birth_data(obj: *const ObjectHeader, data: StateData) -> Option<f64> {
    let index = match data {
        StateData::ReadableHwm => 0,
        StateData::ReadableChunks => 1,
        StateData::ReadableBuffered => 3,
        StateData::Pipes => 4,
        StateData::WritableHwm => 0,
        StateData::WritableLength => 1,
        StateData::WritableCorked => 2,
        StateData::WritableWriteLen => 4,
    };
    let desc = if data.readable() {
        std::ptr::addr_of!(crate::object::runtime_layouts::READABLE_STATE)
    } else {
        std::ptr::addr_of!(crate::object::runtime_layouts::WRITABLE_STATE)
    };
    let shape = unsafe { crate::object::runtime_layouts::layout_id(desc) };
    // SAFETY: the caller validated this live ordinary state receiver.
    if shape == 0 || unsafe { (*obj).parent_class_id } != shape {
        return None;
    }
    // This declared family has all its public data slots inline.
    let bits = unsafe {
        // A1_UNPROVEN: record-first: no proof of the object form at this slot read (kind-A/B question for S0f).
        let fields =
            crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj as *mut _)) as *const u64;
        *fields.add(index)
    };
    Some(f64::from_bits(bits))
}

/// Node's own data properties are the state shape. Its owner is a traced
/// native_state edge, so reflection needs no hidden-key filter.
fn install_state_view(stream: f64, kind: usize, property: crate::runtime_state_key::NamedStateKey) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    // Materialize the shared prototype identity before the newborn escapes.
    let _proto = scope.root_nanbox_f64(state_proto(kind));
    let desc = if kind == READABLE_KIND {
        std::ptr::addr_of!(crate::object::runtime_layouts::READABLE_STATE)
    } else {
        std::ptr::addr_of!(crate::object::runtime_layouts::WRITABLE_STATE)
    };
    let view = scope.root_nanbox_f64(box_pointer(unsafe {
        crate::object::layout_desc::perry_alloc_layout(desc).cast()
    }));
    let obj = object_ptr_from_value(view.get_nanbox_f64()).unwrap();
    unsafe {
        crate::object::object_meta_ensure(
            crate::meta_obj_runtime_born!(obj),
            crate::object::MetaReason::RuntimeHandle,
        )
    };
    let obj = object_ptr_from_value(view.get_nanbox_f64()).unwrap();
    // A1_UNPROVEN: record-first: stream object layout word (kind-A/B question for S0f).
    let meta = unsafe { crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)) };
    let owner = stream.get_nanbox_u64();
    // The existing MetaOnly visitor traces every pointer-tagged native_state.
    unsafe {
        (*meta).native_state = owner;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of_mut!((*meta).native_state) as usize,
            owner,
        );
    }
    let mut values = Vec::new();
    if kind == READABLE_KIND {
        let buffer = scope.root_nanbox_f64(box_pointer(crate::array::js_array_alloc(0).cast()));
        set_hidden_value(
            stream.get_nanbox_f64(),
            StateData::ReadableChunks,
            buffer.get_nanbox_f64(),
        );
        values.extend([
            super::state_record::constructor_data(stream.get_nanbox_f64(), StateData::ReadableHwm)
                .unwrap_or(default_hwm(false)),
            buffer.get_nanbox_f64(),
            0.0,
            0.0,
            super::state_record::constructor_data(stream.get_nanbox_f64(), StateData::Pipes)
                .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
            f64::from_bits(TAG_NULL),
        ]);
    } else {
        let callback =
            scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(state_onwrite, 1), 1));
        callback.with_mut_ptr::<ClosureHeader, _>(|c| {
            js_closure_set_capture_f64(c, 0, stream.get_nanbox_f64())
        });
        values.extend([
            super::state_record::constructor_data(stream.get_nanbox_f64(), StateData::WritableHwm)
                .unwrap_or(default_hwm(false)),
            0.0,
            0.0,
            box_pointer(callback.get_raw_mut_ptr::<ClosureHeader>().cast()),
            0.0,
            0.0,
            0.0,
        ]);
    }
    let values = scope.root_nanbox_f64_slice(&values);
    let words: Vec<f64> = values.iter().map(|v| v.get_nanbox_f64()).collect();
    let obj = object_ptr_from_value(view.get_nanbox_f64()).unwrap();
    for (slot, value) in words.iter().enumerate() {
        crate::object::js_object_set_field_f64(obj, slot as u32, *value);
    }
    super::constructors::birth::stream_birth_field!(
        stream.get_nanbox_f64(),
        property,
        view.get_nanbox_f64()
    );
}

extern "C" fn state_onwrite(c: *const ClosureHeader, _: crate::closure::JsThis, err: f64) -> f64 {
    let stream = js_closure_get_capture_f64(c, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let err = scope.root_nanbox_f64(err);
    let len = get_hidden_value(stream.get_nanbox_f64(), StateData::WritableWriteLen).unwrap_or(0.0);
    let callback = get_hidden_value(stream.get_nanbox_f64(), Slot::WritableWriteCallback)
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    super::write_state::complete_writable_write(
        stream.get_nanbox_f64(),
        len,
        callback,
        err.get_nanbox_f64(),
    );
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn install_readable_state_view(stream: f64) {
    install_state_view(
        stream,
        READABLE_KIND,
        crate::runtime_state_key!(b"_readableState"),
    );
}

pub(super) fn install_writable_state_view(stream: f64) {
    install_state_view(
        stream,
        WRITABLE_KIND,
        crate::runtime_state_key!(b"_writableState"),
    );
}

/// Node sets `closeEmitted` right before `'close'` would be emitted, whether
/// or not `emitClose` lets the event itself out.
pub(super) fn note_close_emitted(stream: f64) {
    set_hidden_value(stream, STREAM_CLOSE_EMITTED_KEY, bool_bits(true));
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn static_layout_state_prototypes_keep_the_declared_shape_and_pairs() {
        crate::gc::initialize_runtime_fixture();
        let scope = crate::gc::RuntimeHandleScope::new();
        for (kind, desc, fields) in [
            (
                READABLE_KIND,
                std::ptr::addr_of!(crate::object::runtime_layouts::READABLE_STATE_PROTO),
                READABLE_FIELDS,
            ),
            (
                WRITABLE_KIND,
                std::ptr::addr_of!(crate::object::runtime_layouts::WRITABLE_STATE_PROTO),
                WRITABLE_FIELDS,
            ),
        ] {
            let proto = scope.root_nanbox_f64(state_proto(kind));
            assert_eq!(proto.get_nanbox_u64(), state_proto(kind).to_bits());
            let ptr =
                crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as *mut ObjectHeader;
            unsafe {
                assert_eq!(
                    (*ptr).parent_class_id,
                    (*desc).layout_id,
                    "accessor installation must not mint another shape"
                );
                assert_eq!(crate::object::object_inline_capacity(ptr), 16);
            }
            for name in fields {
                let attrs = crate::object::get_property_attrs(ptr as usize, name).unwrap();
                assert_eq!(attrs.bits, PropertyAttrs::new(true, false, true).bits);
                let accessor = crate::object::get_accessor_descriptor(ptr as usize, name).unwrap();
                assert_ne!(accessor.get, TAG_UNDEFINED);
                assert_ne!(accessor.set, TAG_UNDEFINED);
            }
            crate::gc::js_gc_collect();
            assert_eq!(proto.get_nanbox_u64(), state_proto(kind).to_bits());
        }
    }
}

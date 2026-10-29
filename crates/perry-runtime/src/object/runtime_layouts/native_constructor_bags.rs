// Native constructor own-property bags. The closure remains an exotic
// function; its bag is an ordinary, null-prototype object with these exact
// runtime-defined keys. Each singleton has its own semantic generation, so
// identical key lists (Transform, PassThrough, the http constructors) still
// name distinct sealed definitions. Its shadow bit covers every later write.
mod native_constructor_atoms {
    use super::*;
    runtime_atom!(NC_ATOM_DUPLEX, "Duplex", "4475706c6578");
    runtime_atom!(
        NC_ATOM_EVENT_EMITTER,
        "EventEmitter",
        "4576656e74456d6974746572"
    );
    runtime_atom!(
        NC_ATOM_EVENT_EMITTER_ASYNC_RESOURCE,
        "EventEmitterAsyncResource",
        "4576656e74456d69747465724173796e635265736f75726365"
    );
    runtime_atom!(
        NC_ATOM_PASS_THROUGH,
        "PassThrough",
        "506173735468726f756768"
    );
    runtime_atom!(NC_ATOM_READABLE, "Readable", "5265616461626c65");
    runtime_atom!(NC_ATOM_STREAM, "Stream", "53747265616d");
    runtime_atom!(NC_ATOM_TRANSFORM, "Transform", "5472616e73666f726d");
    runtime_atom!(NC_ATOM_WRITABLE, "Writable", "5772697461626c65");
    runtime_atom!(
        NC_ATOM__IS_ARRAY_BUFFER_VIEW,
        "_isArrayBufferView",
        "5f6973417272617942756666657256696577"
    );
    runtime_atom!(
        NC_ATOM__IS_UINT8_ARRAY,
        "_isUint8Array",
        "5f697355696e74384172726179"
    );
    runtime_atom!(
        NC_ATOM__UINT8_ARRAY_TO_BUFFER,
        "_uint8ArrayToBuffer",
        "5f75696e74384172726179546f427566666572"
    );
    runtime_atom!(
        NC_ATOM_ADD_ABORT_LISTENER,
        "addAbortListener",
        "61646441626f72744c697374656e6572"
    );
    runtime_atom!(
        NC_ATOM_ADD_ABORT_SIGNAL,
        "addAbortSignal",
        "61646441626f72745369676e616c"
    );
    runtime_atom!(
        NC_ATOM_CAPTURE_REJECTION_SYMBOL,
        "captureRejectionSymbol",
        "6361707475726552656a656374696f6e53796d626f6c"
    );
    runtime_atom!(
        NC_ATOM_CAPTURE_REJECTIONS,
        "captureRejections",
        "6361707475726552656a656374696f6e73"
    );
    runtime_atom!(reuse NC_ATOM_COMPOSE, "compose", "636f6d706f7365");
    runtime_atom!(
        NC_ATOM_DEFAULT_MAX_LISTENERS,
        "defaultMaxListeners",
        "64656661756c744d61784c697374656e657273"
    );
    runtime_atom!(NC_ATOM_DUPLEX_PAIR, "duplexPair", "6475706c657850616972");
    runtime_atom!(
        NC_ATOM_ERROR_MONITOR,
        "errorMonitor",
        "6572726f724d6f6e69746f72"
    );
    runtime_atom!(reuse NC_ATOM_FINISHED, "finished", "66696e6973686564");
    runtime_atom!(NC_ATOM_FROM, "from", "66726f6d");
    runtime_atom!(NC_ATOM_FROM_WEB, "fromWeb", "66726f6d576562");
    runtime_atom!(
        NC_ATOM_GET_DEFAULT_HIGH_WATER_MARK,
        "getDefaultHighWaterMark",
        "67657444656661756c744869676857617465724d61726b"
    );
    runtime_atom!(
        NC_ATOM_GET_EVENT_LISTENERS,
        "getEventListeners",
        "6765744576656e744c697374656e657273"
    );
    runtime_atom!(reuse NC_ATOM_GET_MAX_LISTENERS, "getMaxListeners", "6765744d61784c697374656e657273");
    runtime_atom!(NC_ATOM_INIT, "init", "696e6974");
    runtime_atom!(
        NC_ATOM_IS_DESTROYED,
        "isDestroyed",
        "697344657374726f796564"
    );
    runtime_atom!(
        NC_ATOM_IS_DISTURBED,
        "isDisturbed",
        "6973446973747572626564"
    );
    runtime_atom!(NC_ATOM_IS_ERRORED, "isErrored", "69734572726f726564");
    runtime_atom!(NC_ATOM_IS_READABLE, "isReadable", "69735265616461626c65");
    runtime_atom!(NC_ATOM_IS_WRITABLE, "isWritable", "69735772697461626c65");
    runtime_atom!(reuse NC_ATOM_LENGTH, "length", "6c656e677468");
    runtime_atom!(reuse NC_ATOM_LISTENER_COUNT, "listenerCount", "6c697374656e6572436f756e74");
    runtime_atom!(reuse NC_ATOM_NAME, "name", "6e616d65");
    runtime_atom!(reuse NC_ATOM_ON, "on", "6f6e");
    runtime_atom!(reuse NC_ATOM_ONCE, "once", "6f6e6365");
    runtime_atom!(NC_ATOM_PIPELINE, "pipeline", "706970656c696e65");
    runtime_atom!(NC_ATOM_PROMISES, "promises", "70726f6d69736573");
    runtime_atom!(NC_ATOM_PROTOTYPE, "prototype", "70726f746f74797065");
    runtime_atom!(
        NC_ATOM_SET_DEFAULT_HIGH_WATER_MARK,
        "setDefaultHighWaterMark",
        "73657444656661756c744869676857617465724d61726b"
    );
    runtime_atom!(reuse NC_ATOM_SET_MAX_LISTENERS, "setMaxListeners", "7365744d61784c697374656e657273");
    runtime_atom!(NC_ATOM_TO_WEB, "toWeb", "746f576562");
    runtime_atom!(
        NC_ATOM_USING_DOMAINS,
        "usingDomains",
        "7573696e67446f6d61696e73"
    );
}
use native_constructor_atoms::*;

runtime_layout!(NATIVE_CTOR_EVENT_EMITTER, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0000,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_ADD_ABORT_LISTENER,
    NC_ATOM_ONCE,
    NC_ATOM_ON,
    NC_ATOM_GET_EVENT_LISTENERS,
    NC_ATOM_GET_MAX_LISTENERS,
    NC_ATOM_LISTENER_COUNT,
    NC_ATOM_SET_MAX_LISTENERS,
    NC_ATOM_INIT,
    NC_ATOM_EVENT_EMITTER,
    NC_ATOM_USING_DOMAINS,
    NC_ATOM_CAPTURE_REJECTION_SYMBOL,
    NC_ATOM_ERROR_MONITOR,
    NC_ATOM_EVENT_EMITTER_ASYNC_RESOURCE => E_ACCESSOR | E_HAS_GET | crate::object::key_attrs::ENTRY_NON_WRITABLE,
    NC_ATOM_DEFAULT_MAX_LISTENERS => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_CONFIGURABLE | crate::object::key_attrs::ENTRY_NON_WRITABLE,
    NC_ATOM_CAPTURE_REJECTIONS => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_CONFIGURABLE | crate::object::key_attrs::ENTRY_NON_WRITABLE,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:events.EventEmitter"]);

runtime_layout!(NATIVE_CTOR_STREAM, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0001,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NC_ATOM_IS_DESTROYED,
    NC_ATOM_IS_DISTURBED,
    NC_ATOM_IS_ERRORED,
    NC_ATOM_IS_READABLE,
    NC_ATOM_IS_WRITABLE,
    NC_ATOM_READABLE,
    NC_ATOM_WRITABLE,
    NC_ATOM_DUPLEX,
    NC_ATOM_TRANSFORM,
    NC_ATOM_PASS_THROUGH,
    NC_ATOM_DUPLEX_PAIR,
    NC_ATOM_PIPELINE,
    NC_ATOM_ADD_ABORT_SIGNAL,
    NC_ATOM_FINISHED,
    NC_ATOM_COMPOSE,
    NC_ATOM_SET_DEFAULT_HIGH_WATER_MARK,
    NC_ATOM_GET_DEFAULT_HIGH_WATER_MARK,
    NC_ATOM_PROMISES,
    NC_ATOM_STREAM,
    NC_ATOM__IS_ARRAY_BUFFER_VIEW,
    NC_ATOM__IS_UINT8_ARRAY,
    NC_ATOM__UINT8_ARRAY_TO_BUFFER,
], realm = ["node:stream.Stream"]);

runtime_layout!(NATIVE_CTOR_READABLE, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0002,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NC_ATOM_FROM,
    NC_ATOM_FROM_WEB,
    NC_ATOM_TO_WEB,
], realm = ["node:stream.Readable"]);

runtime_layout!(NATIVE_CTOR_WRITABLE, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0003,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NC_ATOM_FROM_WEB,
    NC_ATOM_TO_WEB,
], realm = ["node:stream.Writable"]);

runtime_layout!(NATIVE_CTOR_DUPLEX, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0004,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NC_ATOM_FROM,
    NC_ATOM_FROM_WEB,
    NC_ATOM_TO_WEB,
], realm = ["node:stream.Duplex"]);

runtime_layout!(NATIVE_CTOR_TRANSFORM, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0005,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:stream.Transform"]);

runtime_layout!(NATIVE_CTOR_PASSTHROUGH, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0006,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:stream.PassThrough"]);

runtime_layout!(NATIVE_CTOR_OUTGOING_MESSAGE, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0007,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:http.OutgoingMessage"]);

runtime_layout!(NATIVE_CTOR_SERVER_RESPONSE, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0008,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:http.ServerResponse"]);

runtime_layout!(NATIVE_CTOR_INCOMING_MESSAGE, kind = LAYOUT_KIND_UNMARKED, 0x4e43_0009,
    crate::object::layout_desc::PROTOTYPE_IMAGE_VARIANT, crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    NC_ATOM_LENGTH => BUILTIN_READ_ONLY,
    NC_ATOM_NAME => BUILTIN_READ_ONLY,
    NC_ATOM_PROTOTYPE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:http.IncomingMessage"]);

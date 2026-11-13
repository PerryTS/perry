//! Reserved prototype identities used by owning constructor declarations.
/// First identity available to non-intrinsic prototypes.
pub const FIRST_DYNAMIC_PROTOTYPE_SERIAL: u64 = 64;

/// Intrinsic-prototype serials (`ObjectMeta.proto_serial`) assigned at
/// creation, so a base shape can name its prototype before the prototype
/// object exists. Dynamic serials start above
/// [`FIRST_DYNAMIC_PROTOTYPE_SERIAL`].
pub const INTRINSIC_SERIAL_FUNCTION: u64 = 1;
pub const INTRINSIC_SERIAL_ASYNC_FUNCTION: u64 = 2;
pub const INTRINSIC_SERIAL_GENERATOR_FUNCTION: u64 = 3;
pub const INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION: u64 = 4;
/// Not a prototype: the marker `proto_id` of the class-constructor ShapeId
/// ([`function_class_shape`]). Shapes are canonical per facts, so without a
/// fact of its own the class shape would BE the FunctionDictionary id. No
/// object is ever assigned this serial; a class constructor's real
/// [[Prototype]] lives on the object (dictionary kind: "ask the object").
pub const INTRINSIC_SERIAL_CLASS_CONSTRUCTOR_MARKER: u64 = 5;
/// Runtime prototypes that runtime layout descriptors name
/// (`runtime_layouts::*_PROTO_FAMILY`).
pub const INTRINSIC_SERIAL_URL: u64 = 6;
pub const INTRINSIC_SERIAL_READABLE_STATE: u64 = 7;
pub const INTRINSIC_SERIAL_WRITABLE_STATE: u64 = 8;
pub const INTRINSIC_SERIAL_TIMEOUT: u64 = 9;
pub const INTRINSIC_SERIAL_IMMEDIATE: u64 = 10;
/// `%Array.prototype%`'s named-property holder (`ArrayHeader.named`, born on
/// `runtime_layouts::ARRAY_PROTOTYPE_NAMED`): its identity word is how an
/// array read reaches it.
pub const INTRINSIC_SERIAL_ARRAY_PROTOTYPE: u64 = 11;
/// `%URLSearchParams.prototype%`, which `runtime_layouts::URL_SEARCH_PARAMS`
/// names through its prototype family.
pub const INTRINSIC_SERIAL_URL_SEARCH_PARAMS: u64 = 12;
/// The prototypes of node:events, node:stream and node:http, born on their
/// static layouts (`runtime_layouts::node_prototypes`): each layout names its
/// [[Prototype]] through the parent's identity word.
pub const INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE: u64 = 13;
pub const INTRINSIC_SERIAL_STREAM_PROTOTYPE: u64 = 14;
pub const INTRINSIC_SERIAL_READABLE_PROTOTYPE: u64 = 15;
pub const INTRINSIC_SERIAL_WRITABLE_PROTOTYPE: u64 = 16;
pub const INTRINSIC_SERIAL_DUPLEX_PROTOTYPE: u64 = 17;
pub const INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE: u64 = 18;
pub const INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE: u64 = 19;
pub const INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE: u64 = 20;
pub const INTRINSIC_SERIAL_INCOMING_MESSAGE_PROTOTYPE: u64 = 21;
pub const INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE: u64 = 22;
/// The sealed own-property bags of native constructors (not the closures).
pub const INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER: u64 = 23;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_STREAM: u64 = 24;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_READABLE: u64 = 25;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_WRITABLE: u64 = 26;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_DUPLEX: u64 = 27;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_TRANSFORM: u64 = 28;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_PASSTHROUGH: u64 = 29;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_OUTGOING_MESSAGE: u64 = 30;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_SERVER_RESPONSE: u64 = 31;
pub const INTRINSIC_SERIAL_NATIVE_CTOR_INCOMING_MESSAGE: u64 = 32;
/// `%DataView.prototype%`, sealed at its birth: its shadow bit is what lets a
/// direct `dv.get*` / `dv.set*` call skip the member read (`buffer::dataview`).
pub const INTRINSIC_SERIAL_DATA_VIEW_PROTOTYPE: u64 = 33;
const _: () = assert!(INTRINSIC_SERIAL_DATA_VIEW_PROTOTYPE < FIRST_DYNAMIC_PROTOTYPE_SERIAL);

/// The existing iterator tower's eight root identities, in its birth order.
/// Reserved serials are facts on those cells, not a cell-to-family registry.
pub const INTRINSIC_SERIAL_ITERATOR_PROTOTYPE: u64 = 34;
pub const INTRINSIC_SERIAL_ARRAY_ITERATOR_PROTOTYPE: u64 = 35;
pub const INTRINSIC_SERIAL_MAP_ITERATOR_PROTOTYPE: u64 = 36;
pub const INTRINSIC_SERIAL_SET_ITERATOR_PROTOTYPE: u64 = 37;
pub const INTRINSIC_SERIAL_STRING_ITERATOR_PROTOTYPE: u64 = 38;
pub const INTRINSIC_SERIAL_REGEXP_STRING_ITERATOR_PROTOTYPE: u64 = 39;
pub const INTRINSIC_SERIAL_ITERATOR_HELPER_PROTOTYPE: u64 = 40;
pub const INTRINSIC_SERIAL_WRAP_FOR_VALID_ITERATOR_PROTOTYPE: u64 = 41;
const _: () =
    assert!(INTRINSIC_SERIAL_WRAP_FOR_VALID_ITERATOR_PROTOTYPE < FIRST_DYNAMIC_PROTOTYPE_SERIAL);

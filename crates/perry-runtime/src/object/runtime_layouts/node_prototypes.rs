// The prototypes of node:events and node:stream as static layouts
// (object model §3.6: builtin prototypes are families with runtime-defined
// static layouts; step 6 slice 3b-ii). Generated from node 26's own-key
// inventory (`Reflect.ownKeys` and the property descriptors of each
// prototype): exactly node's keys, in node's order, with node's attributes.
// Each prototype is born on its layout, old and never moving; population
// fills the declared slots in place (no key add), and the prototype is
// sealed (`builtin_proto::seal_runtime_prototype`), so every later write
// sets its shadow bit. A layout names its [[Prototype]] through the
// parent's runtime prototype family.

const E_NON_ENUMERABLE: u8 = crate::object::key_attrs::ENTRY_NON_ENUMERABLE;
const E_NON_CONFIGURABLE: u8 = crate::object::key_attrs::ENTRY_NON_CONFIGURABLE;
const E_ACCESSOR: u8 = crate::object::key_attrs::ENTRY_ACCESSOR;
const E_HAS_GET: u8 = crate::object::key_attrs::ENTRY_HAS_GET;
const E_HAS_SET: u8 = crate::object::key_attrs::ENTRY_HAS_SET;

runtime_prototype_family!(
    EVENT_EMITTER_PROTO_FAMILY = INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE,
    STREAM_PROTO_FAMILY = INTRINSIC_SERIAL_STREAM_PROTOTYPE,
    READABLE_PROTO_FAMILY = INTRINSIC_SERIAL_READABLE_PROTOTYPE,
    DUPLEX_PROTO_FAMILY = INTRINSIC_SERIAL_DUPLEX_PROTOTYPE,
    OUTGOING_MESSAGE_PROTO_FAMILY = INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE,
    SERVER_RESPONSE_PROTO_FAMILY = INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE,
    PASSTHROUGH_PROTO_FAMILY = INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE,
);

mod node_prototype_atoms {
    use super::*;
    runtime_atom!(
        NP_ATOM_ADD_LISTENER,
        "addListener",
        "6164644c697374656e6572"
    );
    runtime_atom!(
        NP_ATOM_ADD_TRAILERS,
        "addTrailers",
        "616464547261696c657273"
    );
    runtime_atom!(
        NP_ATOM_APPEND_HEADER,
        "appendHeader",
        "617070656e64486561646572"
    );
    runtime_atom!(
        NP_ATOM_ASSIGN_SOCKET,
        "assignSocket",
        "61737369676e536f636b6574"
    );
    runtime_atom!(reuse NP_ATOM_CLOSED, "closed", "636c6f736564");
    runtime_atom!(NP_ATOM_COMPOSE, "compose", "636f6d706f7365");
    runtime_atom!(NP_ATOM_CONNECTION, "connection", "636f6e6e656374696f6e");
    runtime_atom!(reuse NP_ATOM_CONSTRUCTOR, "constructor", "636f6e7374727563746f72");
    runtime_atom!(NP_ATOM_CORK, "cork", "636f726b");
    runtime_atom!(NP_ATOM_DESTROY, "destroy", "64657374726f79");
    runtime_atom!(reuse NP_ATOM_DESTROYED, "destroyed", "64657374726f796564");
    runtime_atom!(
        NP_ATOM_DETACH_SOCKET,
        "detachSocket",
        "646574616368536f636b6574"
    );
    runtime_atom!(reuse NP_ATOM_DROP, "drop", "64726f70");
    runtime_atom!(reuse NP_ATOM_EMIT, "emit", "656d6974");
    runtime_atom!(NP_ATOM_END, "end", "656e64");
    runtime_atom!(reuse NP_ATOM_ERRORED, "errored", "6572726f726564");
    runtime_atom!(NP_ATOM_EVENT_NAMES, "eventNames", "6576656e744e616d6573");
    runtime_atom!(reuse NP_ATOM_EVERY, "every", "6576657279");
    runtime_atom!(reuse NP_ATOM_FILTER, "filter", "66696c746572");
    runtime_atom!(reuse NP_ATOM_FIND, "find", "66696e64");
    runtime_atom!(reuse NP_ATOM_FLAT_MAP, "flatMap", "666c61744d6170");
    runtime_atom!(
        NP_ATOM_FLUSH_HEADERS,
        "flushHeaders",
        "666c75736848656164657273"
    );
    runtime_atom!(reuse NP_ATOM_FOR_EACH, "forEach", "666f7245616368");
    runtime_atom!(NP_ATOM_GET_HEADER, "getHeader", "676574486561646572");
    runtime_atom!(NP_ATOM_GET_HEADERS, "getHeaders", "67657448656164657273");
    runtime_atom!(
        NP_ATOM_GET_HEADER_NAMES,
        "getHeaderNames",
        "6765744865616465724e616d6573"
    );
    runtime_atom!(
        NP_ATOM_GET_MAX_LISTENERS,
        "getMaxListeners",
        "6765744d61784c697374656e657273"
    );
    runtime_atom!(
        NP_ATOM_GET_RAW_HEADER_NAMES,
        "getRawHeaderNames",
        "6765745261774865616465724e616d6573"
    );
    runtime_atom!(NP_ATOM_HAS_HEADER, "hasHeader", "686173486561646572");
    runtime_atom!(NP_ATOM_HEADERS, "headers", "68656164657273");
    runtime_atom!(
        NP_ATOM_HEADERS_DISTINCT,
        "headersDistinct",
        "6865616465727344697374696e6374"
    );
    runtime_atom!(
        NP_ATOM_HEADERS_SENT,
        "headersSent",
        "6865616465727353656e74"
    );
    runtime_atom!(NP_ATOM_IS_PAUSED, "isPaused", "6973506175736564");
    runtime_atom!(NP_ATOM_ITERATOR, "iterator", "6974657261746f72");
    runtime_atom!(NP_ATOM_LISTENERS, "listeners", "6c697374656e657273");
    runtime_atom!(
        NP_ATOM_LISTENER_COUNT,
        "listenerCount",
        "6c697374656e6572436f756e74"
    );
    runtime_atom!(reuse NP_ATOM_MAP, "map", "6d6170");
    runtime_atom!(NP_ATOM_OFF, "off", "6f6666");
    runtime_atom!(NP_ATOM_ON, "on", "6f6e");
    runtime_atom!(NP_ATOM_ONCE, "once", "6f6e6365");
    runtime_atom!(NP_ATOM_PAUSE, "pause", "7061757365");
    runtime_atom!(NP_ATOM_PIPE, "pipe", "70697065");
    runtime_atom!(
        NP_ATOM_PREPEND_LISTENER,
        "prependListener",
        "70726570656e644c697374656e6572"
    );
    runtime_atom!(
        NP_ATOM_PREPEND_ONCE_LISTENER,
        "prependOnceListener",
        "70726570656e644f6e63654c697374656e6572"
    );
    runtime_atom!(reuse NP_ATOM_PUSH, "push", "70757368");
    runtime_atom!(
        NP_ATOM_RAW_LISTENERS,
        "rawListeners",
        "7261774c697374656e657273"
    );
    runtime_atom!(NP_ATOM_READ, "read", "72656164");
    runtime_atom!(NP_ATOM_READABLE, "readable", "7265616461626c65");
    runtime_atom!(
        NP_ATOM_READABLE_ABORTED,
        "readableAborted",
        "7265616461626c6541626f72746564"
    );
    runtime_atom!(
        NP_ATOM_READABLE_BUFFER,
        "readableBuffer",
        "7265616461626c65427566666572"
    );
    runtime_atom!(
        NP_ATOM_READABLE_DID_READ,
        "readableDidRead",
        "7265616461626c6544696452656164"
    );
    runtime_atom!(
        NP_ATOM_READABLE_ENCODING,
        "readableEncoding",
        "7265616461626c65456e636f64696e67"
    );
    runtime_atom!(
        NP_ATOM_READABLE_ENDED,
        "readableEnded",
        "7265616461626c65456e646564"
    );
    runtime_atom!(
        NP_ATOM_READABLE_FLOWING,
        "readableFlowing",
        "7265616461626c65466c6f77696e67"
    );
    runtime_atom!(
        NP_ATOM_READABLE_HIGH_WATER_MARK,
        "readableHighWaterMark",
        "7265616461626c654869676857617465724d61726b"
    );
    runtime_atom!(
        NP_ATOM_READABLE_LENGTH,
        "readableLength",
        "7265616461626c654c656e677468"
    );
    runtime_atom!(
        NP_ATOM_READABLE_OBJECT_MODE,
        "readableObjectMode",
        "7265616461626c654f626a6563744d6f6465"
    );
    runtime_atom!(reuse NP_ATOM_REDUCE, "reduce", "726564756365");
    runtime_atom!(
        NP_ATOM_REMOVE_ALL_LISTENERS,
        "removeAllListeners",
        "72656d6f7665416c6c4c697374656e657273"
    );
    runtime_atom!(
        NP_ATOM_REMOVE_HEADER,
        "removeHeader",
        "72656d6f7665486561646572"
    );
    runtime_atom!(
        NP_ATOM_REMOVE_LISTENER,
        "removeListener",
        "72656d6f76654c697374656e6572"
    );
    runtime_atom!(NP_ATOM_RESUME, "resume", "726573756d65");
    runtime_atom!(
        NP_ATOM_SET_DEFAULT_ENCODING,
        "setDefaultEncoding",
        "73657444656661756c74456e636f64696e67"
    );
    runtime_atom!(
        NP_ATOM_SET_ENCODING,
        "setEncoding",
        "736574456e636f64696e67"
    );
    runtime_atom!(NP_ATOM_SET_HEADER, "setHeader", "736574486561646572");
    runtime_atom!(NP_ATOM_SET_HEADERS, "setHeaders", "73657448656164657273");
    runtime_atom!(
        NP_ATOM_SET_MAX_LISTENERS,
        "setMaxListeners",
        "7365744d61784c697374656e657273"
    );
    runtime_atom!(NP_ATOM_SET_TIMEOUT, "setTimeout", "73657454696d656f7574");
    runtime_atom!(reuse NP_ATOM_SIGNAL, "signal", "7369676e616c");
    runtime_atom!(NP_ATOM_SOCKET, "socket", "736f636b6574");
    runtime_atom!(reuse NP_ATOM_SOME, "some", "736f6d65");
    runtime_atom!(NP_ATOM_STATUS_CODE, "statusCode", "737461747573436f6465");
    runtime_atom!(
        NP_ATOM_STATUS_MESSAGE,
        "statusMessage",
        "7374617475734d657373616765"
    );
    runtime_atom!(NP_ATOM_TAKE, "take", "74616b65");
    runtime_atom!(NP_ATOM_TO_ARRAY, "toArray", "746f4172726179");
    runtime_atom!(NP_ATOM_TRAILERS, "trailers", "747261696c657273");
    runtime_atom!(
        NP_ATOM_TRAILERS_DISTINCT,
        "trailersDistinct",
        "747261696c65727344697374696e6374"
    );
    runtime_atom!(NP_ATOM_UNCORK, "uncork", "756e636f726b");
    runtime_atom!(NP_ATOM_UNPIPE, "unpipe", "756e70697065");
    runtime_atom!(reuse NP_ATOM_UNSHIFT, "unshift", "756e7368696674");
    runtime_atom!(
        NP_ATOM_U_ADD_HEADER_LINE,
        "_addHeaderLine",
        "5f6164644865616465724c696e65"
    );
    runtime_atom!(
        NP_ATOM_U_ADD_HEADER_LINES,
        "_addHeaderLines",
        "5f6164644865616465724c696e6573"
    );
    runtime_atom!(
        NP_ATOM_U_ADD_HEADER_LINE_DISTINCT,
        "_addHeaderLineDistinct",
        "5f6164644865616465724c696e6544697374696e6374"
    );
    runtime_atom!(NP_ATOM_U_DESTROY, "_destroy", "5f64657374726f79");
    runtime_atom!(NP_ATOM_U_DUMP, "_dump", "5f64756d70");
    runtime_atom!(
        NP_ATOM_U_DUMP_AND_CLOSE_READABLE,
        "_dumpAndCloseReadable",
        "5f64756d70416e64436c6f73655265616461626c65"
    );
    runtime_atom!(NP_ATOM_U_EVENTS, "_events", "5f6576656e7473");
    runtime_atom!(
        NP_ATOM_U_EVENTS_COUNT,
        "_eventsCount",
        "5f6576656e7473436f756e74"
    );
    runtime_atom!(NP_ATOM_U_FINAL, "_final", "5f66696e616c");
    runtime_atom!(NP_ATOM_U_FINISH, "_finish", "5f66696e697368");
    runtime_atom!(NP_ATOM_U_FLUSH, "_flush", "5f666c757368");
    runtime_atom!(
        NP_ATOM_U_FLUSH_OUTPUT,
        "_flushOutput",
        "5f666c7573684f7574707574"
    );
    runtime_atom!(
        NP_ATOM_U_IMPLICIT_HEADER,
        "_implicitHeader",
        "5f696d706c69636974486561646572"
    );
    runtime_atom!(
        NP_ATOM_U_IS_LENIENT_HEADER_VALIDATION,
        "_isLenientHeaderValidation",
        "5f69734c656e69656e7448656164657256616c69646174696f6e"
    );
    runtime_atom!(
        NP_ATOM_U_MAX_LISTENERS,
        "_maxListeners",
        "5f6d61784c697374656e657273"
    );
    runtime_atom!(NP_ATOM_U_READ, "_read", "5f72656164");
    runtime_atom!(
        NP_ATOM_U_RENDER_HEADERS,
        "_renderHeaders",
        "5f72656e64657248656164657273"
    );
    runtime_atom!(NP_ATOM_U_SEND, "_send", "5f73656e64");
    runtime_atom!(
        NP_ATOM_U_STORE_HEADER,
        "_storeHeader",
        "5f73746f7265486561646572"
    );
    runtime_atom!(NP_ATOM_U_TRANSFORM, "_transform", "5f7472616e73666f726d");
    runtime_atom!(NP_ATOM_U_UNDESTROY, "_undestroy", "5f756e64657374726f79");
    runtime_atom!(NP_ATOM_U_WRITE, "_write", "5f7772697465");
    runtime_atom!(NP_ATOM_U_WRITEV, "_writev", "5f777269746576");
    runtime_atom!(NP_ATOM_U_WRITE_RAW, "_writeRaw", "5f7772697465526177");
    runtime_atom!(NP_ATOM_WRAP, "wrap", "77726170");
    runtime_atom!(reuse NP_ATOM_WRITABLE, "writable", "7772697461626c65");
    runtime_atom!(
        NP_ATOM_WRITABLE_ABORTED,
        "writableAborted",
        "7772697461626c6541626f72746564"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_BUFFER,
        "writableBuffer",
        "7772697461626c65427566666572"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_CORKED,
        "writableCorked",
        "7772697461626c65436f726b6564"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_ENDED,
        "writableEnded",
        "7772697461626c65456e646564"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_FINISHED,
        "writableFinished",
        "7772697461626c6546696e6973686564"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_HIGH_WATER_MARK,
        "writableHighWaterMark",
        "7772697461626c654869676857617465724d61726b"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_LENGTH,
        "writableLength",
        "7772697461626c654c656e677468"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_NEED_DRAIN,
        "writableNeedDrain",
        "7772697461626c654e656564447261696e"
    );
    runtime_atom!(
        NP_ATOM_WRITABLE_OBJECT_MODE,
        "writableObjectMode",
        "7772697461626c654f626a6563744d6f6465"
    );
    runtime_atom!(NP_ATOM_WRITE, "write", "7772697465");
    runtime_atom!(
        NP_ATOM_WRITE_CONTINUE,
        "writeContinue",
        "7772697465436f6e74696e7565"
    );
    runtime_atom!(
        NP_ATOM_WRITE_EARLY_HINTS,
        "writeEarlyHints",
        "77726974654561726c7948696e7473"
    );
    runtime_atom!(NP_ATOM_WRITE_HEAD, "writeHead", "777269746548656164");
    runtime_atom!(
        NP_ATOM_WRITE_INFORMATION,
        "writeInformation",
        "7772697465496e666f726d6174696f6e"
    );
    runtime_atom!(
        NP_ATOM_WRITE_PROCESSING,
        "writeProcessing",
        "777269746550726f63657373696e67"
    );
}
pub(crate) use node_prototype_atoms::*;

/// `Symbol.asyncIterator` as a layout key (a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[linkage = "weak_odr"]
#[export_name = "perry_atom_well_known_asyncIterator"]
pub(crate) static NP_ATOM_SYMBOL_ASYNC_ITERATOR: StaticAtom = StaticAtom {
    hash: crate::codegen_abi::well_known_symbol_atom_hash(1),
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
    _pad: [0; 3],
    index: 1,
};
wasm_atom!(NP_ATOM_SYMBOL_ASYNC_ITERATOR);
const _: () = assert!(
    crate::codegen_abi::WELL_KNOWN_SYMBOLS[1].len() == 13,
    "`asyncIterator` is well-known symbol 1"
);

/// `Symbol.asyncDispose` as a layout key (a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[linkage = "weak_odr"]
#[export_name = "perry_atom_well_known_asyncDispose"]
pub(crate) static NP_ATOM_SYMBOL_ASYNC_DISPOSE: StaticAtom = StaticAtom {
    hash: crate::codegen_abi::well_known_symbol_atom_hash(14),
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
    _pad: [0; 3],
    index: 14,
};
wasm_atom!(NP_ATOM_SYMBOL_ASYNC_DISPOSE);
const _: () = assert!(
    crate::codegen_abi::WELL_KNOWN_SYMBOLS[14].len() == 12,
    "`asyncDispose` is well-known symbol 14"
);

/// The hash of runtime symbol atom `n` (a fixed constant per number, like a
/// well-known symbol's; the symbol itself is created when it is bound).
const fn runtime_symbol_atom_hash(n: u32) -> u64 {
    0x9e37_79b9_7f4a_7c15u64.wrapping_mul(n as u64 + 0x100)
}

/// A symbol of node's own library as a layout key: a kind-2 atom (`string` its
/// description, `index` its number among [`RUNTIME_SYMBOL_ATOMS`]), bound to
/// its process-lifetime symbol by [`runtime_symbol_of_atom`].
macro_rules! runtime_symbol_atom {
    ($name:ident, $desc:literal, $n:expr) => {
        #[allow(non_snake_case)]
        pub(crate) mod $name {
            use super::*;
            pub(crate) const HASH: u64 = runtime_symbol_atom_hash($n);
            pub(super) static IMAGE: ImageString<{ $desc.len() + 1 }> =
                image_bytes(concat!($desc, "\0").as_bytes());
        }
        pub(crate) static $name: StaticAtom = StaticAtom {
            hash: $name::HASH,
            string: std::ptr::addr_of!($name::IMAGE.string),
            kind: crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL,
            _pad: [0; 3],
            index: $n,
        };
    };
}
runtime_symbol_atom!(NP_SYMBOL_K_CAPTURE, "kCapture", 0);
runtime_symbol_atom!(NP_SYMBOL_NODEJS_REJECTION, "nodejs.rejection", 1);
runtime_symbol_atom!(NP_SYMBOL_TO_ASYNC_STREAMABLE, "Stream.toAsyncStreamable", 2);
runtime_symbol_atom!(NP_SYMBOL_K_ATTACH_ABORT_SIGNAL, "kAttachAbortSignal", 3);
runtime_symbol_atom!(NP_SYMBOL_K_DETACH_ABORT_SIGNAL, "kDetachAbortSignal", 4);

runtime_symbol_atom!(NP_SYMBOL_K_HEADERS, "kHeaders", 5);
runtime_symbol_atom!(NP_SYMBOL_K_HEADERS_COUNT, "kHeadersCount", 6);
runtime_symbol_atom!(NP_SYMBOL_K_TRAILERS, "kTrailers", 7);
runtime_symbol_atom!(NP_SYMBOL_K_TRAILERS_COUNT, "kTrailersCount", 8);
runtime_symbol_atom!(NP_SYMBOL_K_ABORT_CONTROLLER, "kAbortController", 9);
runtime_symbol_atom!(NP_SYMBOL_K_ABORT_SIGNAL_SOCKET, "kAbortSignalSocket", 10);
runtime_symbol_atom!(
    NP_SYMBOL_K_ABORT_SIGNAL_LISTENER,
    "kAbortSignalListener",
    11
);
runtime_symbol_atom!(
    NP_SYMBOL_K_ABORT_SIGNAL_DETACHED,
    "kAbortSignalDetached",
    12
);

/// The runtime symbol atoms by number, and whether each is a registry symbol
/// (`Symbol.for(description)`) rather than a symbol of its own.
static RUNTIME_SYMBOL_ATOMS: [(&StaticAtom, bool); 13] = [
    (&NP_SYMBOL_K_CAPTURE, false),
    (&NP_SYMBOL_NODEJS_REJECTION, true),
    (&NP_SYMBOL_TO_ASYNC_STREAMABLE, true),
    (&NP_SYMBOL_K_ATTACH_ABORT_SIGNAL, false),
    (&NP_SYMBOL_K_DETACH_ABORT_SIGNAL, false),
    (&NP_SYMBOL_K_HEADERS, false),
    (&NP_SYMBOL_K_HEADERS_COUNT, false),
    (&NP_SYMBOL_K_TRAILERS, false),
    (&NP_SYMBOL_K_TRAILERS_COUNT, false),
    (&NP_SYMBOL_K_ABORT_CONTROLLER, false),
    (&NP_SYMBOL_K_ABORT_SIGNAL_SOCKET, false),
    (&NP_SYMBOL_K_ABORT_SIGNAL_LISTENER, false),
    (&NP_SYMBOL_K_ABORT_SIGNAL_DETACHED, false),
];
static RUNTIME_SYMBOLS: [std::sync::atomic::AtomicUsize; 13] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 13];

/// The symbol runtime symbol atom `atom` names: created once per process, the
/// same symbol in every agent (registration binds every layout key to it),
/// or `None` for any other atom.
///
/// # Safety
/// `atom` is a static atom.
pub(crate) unsafe fn runtime_symbol_of_atom(
    atom: *const StaticAtom,
) -> Option<*mut crate::symbol::SymbolHeader> {
    use std::sync::atomic::Ordering;
    if (*atom).kind != crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL {
        return None;
    }
    let n = (*atom).index as usize;
    let &(ours, registered) = RUNTIME_SYMBOL_ATOMS.get(n)?;
    if !std::ptr::eq(ours, atom) {
        return None;
    }
    let bound = RUNTIME_SYMBOLS[n].load(Ordering::Acquire);
    if bound != 0 {
        return Some(bound as *mut crate::symbol::SymbolHeader);
    }
    let text = std::slice::from_raw_parts(
        crate::string::string_data(ours.string),
        (*ours.string).byte_len as usize,
    );
    let symbol = if registered {
        let key = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = crate::symbol::js_symbol_for(f64::from_bits(
            crate::value::JSValue::string_ptr(key).bits(),
        ));
        (value.to_bits() & crate::value::POINTER_MASK) as *mut crate::symbol::SymbolHeader
    } else {
        crate::symbol::persistent_symbol(std::str::from_utf8_unchecked(text))
    };
    match RUNTIME_SYMBOLS[n].compare_exchange(
        0,
        symbol as usize,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Some(symbol),
        Err(other) => Some(other as *mut crate::symbol::SymbolHeader),
    }
}

/// Is `ptr` one of the runtime symbols bound so far? Their immortal owner,
/// the way the registry and the well-known cache own theirs
/// (`symbol::is_persistent_symbol`).
pub(crate) fn is_runtime_symbol(ptr: usize) -> bool {
    ptr != 0
        && RUNTIME_SYMBOLS
            .iter()
            .any(|bound| bound.load(std::sync::atomic::Ordering::Acquire) == ptr)
}

/// The NaN-boxed symbol runtime symbol atom `atom` names.
pub(crate) fn runtime_symbol_value(atom: &'static StaticAtom) -> f64 {
    // SAFETY: a static atom.
    match unsafe { runtime_symbol_of_atom(atom) } {
        Some(symbol) => crate::value::js_nanbox_pointer(symbol as i64),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

// `EventEmitter.prototype`: node's own keys, in node's order.
runtime_layout!(EVENT_EMITTER_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500010, 0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_U_EVENTS,
    NP_ATOM_U_EVENTS_COUNT,
    NP_ATOM_U_MAX_LISTENERS,
    NP_ATOM_SET_MAX_LISTENERS,
    NP_ATOM_GET_MAX_LISTENERS,
    NP_ATOM_EMIT,
    NP_ATOM_ADD_LISTENER,
    NP_ATOM_ON,
    NP_ATOM_PREPEND_LISTENER,
    NP_ATOM_ONCE,
    NP_ATOM_PREPEND_ONCE_LISTENER,
    NP_ATOM_REMOVE_LISTENER,
    NP_ATOM_OFF,
    NP_ATOM_REMOVE_ALL_LISTENERS,
    NP_ATOM_LISTENERS,
    NP_ATOM_RAW_LISTENERS,
    NP_ATOM_LISTENER_COUNT,
    NP_ATOM_EVENT_NAMES,
    NP_SYMBOL_K_CAPTURE => E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:events.EventEmitter.prototype"]);

// `Stream.prototype`: node's own keys, in node's order.
runtime_layout!(STREAM_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500011, 0,
    &raw const EVENT_EMITTER_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_PIPE,
    NP_ATOM_EVENT_NAMES,
], realm = ["node:stream.Stream.prototype"]);

// `Readable.prototype`: node's own keys, in node's order.
runtime_layout!(READABLE_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500012, 0,
    &raw const STREAM_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_DESTROY,
    NP_ATOM_U_UNDESTROY,
    NP_ATOM_U_DESTROY,
    NP_ATOM_PUSH,
    NP_ATOM_UNSHIFT,
    NP_ATOM_IS_PAUSED,
    NP_ATOM_SET_ENCODING,
    NP_ATOM_READ,
    NP_ATOM_U_READ,
    NP_ATOM_PIPE,
    NP_ATOM_UNPIPE,
    NP_ATOM_ON,
    NP_ATOM_ADD_LISTENER,
    NP_ATOM_REMOVE_LISTENER,
    NP_ATOM_OFF,
    NP_ATOM_REMOVE_ALL_LISTENERS,
    NP_ATOM_RESUME,
    NP_ATOM_PAUSE,
    NP_ATOM_WRAP,
    NP_ATOM_ITERATOR,
    NP_ATOM_COMPOSE,
    NP_ATOM_READABLE => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_DID_READ => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_ABORTED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_HIGH_WATER_MARK => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_BUFFER => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_FLOWING => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_LENGTH => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_OBJECT_MODE => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_ENCODING => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_ERRORED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_CLOSED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_DESTROYED => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_READABLE_ENDED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_DROP => E_NON_ENUMERABLE,
    NP_ATOM_FILTER => E_NON_ENUMERABLE,
    NP_ATOM_FLAT_MAP => E_NON_ENUMERABLE,
    NP_ATOM_MAP => E_NON_ENUMERABLE,
    NP_ATOM_TAKE => E_NON_ENUMERABLE,
    NP_ATOM_EVERY => E_NON_ENUMERABLE,
    NP_ATOM_FOR_EACH => E_NON_ENUMERABLE,
    NP_ATOM_REDUCE => E_NON_ENUMERABLE,
    NP_ATOM_TO_ARRAY => E_NON_ENUMERABLE,
    NP_ATOM_SOME => E_NON_ENUMERABLE,
    NP_ATOM_FIND => E_NON_ENUMERABLE,
    NP_SYMBOL_NODEJS_REJECTION,
    NP_ATOM_SYMBOL_ASYNC_DISPOSE,
    NP_ATOM_SYMBOL_ASYNC_ITERATOR,
    NP_SYMBOL_TO_ASYNC_STREAMABLE,
], realm = ["node:stream.Readable.prototype"]);

// `Writable.prototype`: node's own keys, in node's order.
runtime_layout!(WRITABLE_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500013, 0,
    &raw const STREAM_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_PIPE,
    NP_ATOM_WRITE,
    NP_ATOM_CORK,
    NP_ATOM_UNCORK,
    NP_ATOM_SET_DEFAULT_ENCODING,
    NP_ATOM_U_WRITE,
    NP_ATOM_U_WRITEV,
    NP_ATOM_END,
    NP_ATOM_CLOSED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_DESTROYED => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_FINISHED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_OBJECT_MODE => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_BUFFER => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_ENDED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_NEED_DRAIN => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_HIGH_WATER_MARK => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_CORKED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_LENGTH => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_ERRORED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_ABORTED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_DESTROY,
    NP_ATOM_U_UNDESTROY,
    NP_ATOM_U_DESTROY,
    NP_SYMBOL_NODEJS_REJECTION,
    NP_ATOM_SYMBOL_ASYNC_DISPOSE,
], realm = ["node:stream.Writable.prototype"]);

// `Duplex.prototype`: node's own keys, in node's order.
runtime_layout!(DUPLEX_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500014, 0,
    &raw const READABLE_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_WRITE,
    NP_ATOM_CORK,
    NP_ATOM_UNCORK,
    NP_ATOM_SET_DEFAULT_ENCODING,
    NP_ATOM_U_WRITE,
    NP_ATOM_U_WRITEV,
    NP_ATOM_END,
    NP_ATOM_DESTROY,
    NP_ATOM_WRITABLE => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_HIGH_WATER_MARK => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_OBJECT_MODE => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_BUFFER => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_LENGTH => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_FINISHED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_CORKED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_ENDED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_NEED_DRAIN => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_DESTROYED => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
], realm = ["node:stream.Duplex.prototype"]);

// `Transform.prototype`: node's own keys, in node's order.
runtime_layout!(TRANSFORM_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500015, 0,
    &raw const DUPLEX_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_U_FINAL,
    NP_ATOM_U_TRANSFORM,
    NP_ATOM_U_WRITE,
    NP_ATOM_U_READ,
], realm = ["node:stream.Transform.prototype"]);
// `OutgoingMessage.prototype`: node's own keys, in node's order.
runtime_layout!(OUTGOING_MESSAGE_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500016, 0,
    &raw const STREAM_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_U_IS_LENIENT_HEADER_VALIDATION,
    NP_ATOM_ERRORED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_CLOSED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_FINISHED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_OBJECT_MODE => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_LENGTH => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_HIGH_WATER_MARK => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_CORKED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_CONNECTION => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_SOCKET => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_U_RENDER_HEADERS,
    NP_ATOM_CORK,
    NP_ATOM_UNCORK,
    NP_ATOM_SET_TIMEOUT,
    NP_ATOM_DESTROY,
    NP_ATOM_U_SEND,
    NP_ATOM_U_WRITE_RAW,
    NP_ATOM_U_STORE_HEADER,
    NP_ATOM_SET_HEADER,
    NP_ATOM_SET_HEADERS,
    NP_ATOM_APPEND_HEADER,
    NP_ATOM_GET_HEADER,
    NP_ATOM_GET_HEADER_NAMES,
    NP_ATOM_GET_RAW_HEADER_NAMES,
    NP_ATOM_GET_HEADERS,
    NP_ATOM_HAS_HEADER,
    NP_ATOM_REMOVE_HEADER,
    NP_ATOM_U_IMPLICIT_HEADER,
    NP_ATOM_HEADERS_SENT => E_ACCESSOR | E_HAS_GET,
    NP_ATOM_WRITABLE_ENDED => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITABLE_NEED_DRAIN => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_WRITE,
    NP_ATOM_ADD_TRAILERS,
    NP_ATOM_END,
    NP_ATOM_U_FINISH,
    NP_ATOM_U_FLUSH,
    NP_ATOM_U_FLUSH_OUTPUT,
    NP_ATOM_FLUSH_HEADERS,
    NP_ATOM_PIPE,
    NP_SYMBOL_NODEJS_REJECTION,
], realm = ["node:http.OutgoingMessage.prototype"]);

// `ServerResponse.prototype`: node's own keys, in node's order.
runtime_layout!(SERVER_RESPONSE_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500017, 0,
    &raw const OUTGOING_MESSAGE_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_U_FINISH,
    NP_ATOM_STATUS_CODE,
    NP_ATOM_STATUS_MESSAGE,
    NP_ATOM_ASSIGN_SOCKET,
    NP_ATOM_DETACH_SOCKET,
    NP_ATOM_WRITE_INFORMATION,
    NP_ATOM_WRITE_CONTINUE,
    NP_ATOM_WRITE_PROCESSING,
    NP_ATOM_WRITE_EARLY_HINTS,
    NP_ATOM_U_IMPLICIT_HEADER,
    NP_ATOM_WRITE_HEAD,
], realm = ["node:http.ServerResponse.prototype"]);

// `IncomingMessage.prototype`: node's own keys, in node's order.
runtime_layout!(INCOMING_MESSAGE_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500018, 0,
    &raw const READABLE_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_CONNECTION => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_HEADERS => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_HEADERS_DISTINCT => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_TRAILERS => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_TRAILERS_DISTINCT => E_ACCESSOR | E_HAS_GET | E_HAS_SET | E_NON_ENUMERABLE | E_NON_CONFIGURABLE,
    NP_ATOM_SIGNAL => E_ACCESSOR | E_HAS_GET | E_NON_ENUMERABLE,
    NP_ATOM_SET_TIMEOUT,
    NP_ATOM_U_READ,
    NP_ATOM_U_DESTROY,
    NP_ATOM_U_ADD_HEADER_LINES,
    NP_ATOM_U_ADD_HEADER_LINE,
    NP_ATOM_U_ADD_HEADER_LINE_DISTINCT,
    NP_ATOM_U_DUMP_AND_CLOSE_READABLE,
    NP_ATOM_U_DUMP,
    NP_SYMBOL_K_ATTACH_ABORT_SIGNAL,
    NP_SYMBOL_K_DETACH_ABORT_SIGNAL,
], realm = ["node:http.IncomingMessage.prototype"]);

// `PassThrough.prototype`: node's own keys, in node's order.
runtime_layout!(PASSTHROUGH_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x53500019, 0,
    &raw const TRANSFORM_PROTO_FAMILY, [
    NP_ATOM_CONSTRUCTOR => E_NON_ENUMERABLE,
    NP_ATOM_U_TRANSFORM,
], realm = ["node:stream.PassThrough.prototype"]);

// Constructor own-property declarations share this runtime family inventory.
include!("native_constructor_bags.rs");

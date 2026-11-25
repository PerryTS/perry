//! Native instance layout of node's `ServerResponse` (object model §3.2,
//! step 6 slice 3b-ii, http half). The key sequence is node's enumeration
//! order: the emitter's three slots, `OutgoingMessage`'s 24 fields, then
//! `ServerResponse`'s own. Members node creates only later (`statusCode` and
//! `statusMessage` at `writeHead`, `_maxRequestsPerSocket` on a server
//! response) are optional: their slot holds the hole until filled, and a
//! fill appends when a later key is present, as node's `[[Set]]` does
//! (`key_view::fill_hole`). The layout names `ServerResponse.prototype`, so a
//! direct `new ServerResponse(req)` is born on it with no prototype link to
//! stamp.
use super::*;

runtime_atom!(HI_ATOM_OUTPUT_DATA, "outputData", "6f757470757444617461");
runtime_atom!(HI_ATOM_OUTPUT_SIZE, "outputSize", "6f757470757453697a65");
runtime_atom!(HI_ATOM_U_LAST, "_last", "5f6c617374");
runtime_atom!(
    HI_ATOM_CHUNKED_ENCODING,
    "chunkedEncoding",
    "6368756e6b6564456e636f64696e67"
);
runtime_atom!(
    HI_ATOM_SHOULD_KEEP_ALIVE,
    "shouldKeepAlive",
    "73686f756c644b656570416c697665"
);
runtime_atom!(
    HI_ATOM_MAX_REQUESTS_ON_CONNECTION_REACHED,
    "maxRequestsOnConnectionReached",
    "6d617852657175657374734f6e436f6e6e656374696f6e52656163686564"
);
runtime_atom!(
    HI_ATOM_U_DEFAULT_KEEP_ALIVE,
    "_defaultKeepAlive",
    "5f64656661756c744b656570416c697665"
);
runtime_atom!(
    HI_ATOM_USE_CHUNKED_ENCODING_BY_DEFAULT,
    "useChunkedEncodingByDefault",
    "7573654368756e6b6564456e636f64696e67427944656661756c74"
);
runtime_atom!(HI_ATOM_SEND_DATE, "sendDate", "73656e6444617465");
runtime_atom!(
    HI_ATOM_U_REMOVED_CONNECTION,
    "_removedConnection",
    "5f72656d6f766564436f6e6e656374696f6e"
);
runtime_atom!(
    HI_ATOM_U_REMOVED_CONT_LEN,
    "_removedContLen",
    "5f72656d6f766564436f6e744c656e"
);
runtime_atom!(HI_ATOM_U_REMOVED_TE, "_removedTE", "5f72656d6f7665645445");
runtime_atom!(
    HI_ATOM_STRICT_CONTENT_LENGTH,
    "strictContentLength",
    "737472696374436f6e74656e744c656e677468"
);
runtime_atom!(
    HI_ATOM_U_CONTENT_LENGTH,
    "_contentLength",
    "5f636f6e74656e744c656e677468"
);
runtime_atom!(HI_ATOM_U_HAS_BODY, "_hasBody", "5f686173426f6479");
runtime_atom!(HI_ATOM_U_TRAILER, "_trailer", "5f747261696c6572");
runtime_atom!(reuse HI_ATOM_FINISHED, "finished", "66696e6973686564");
runtime_atom!(
    HI_ATOM_U_HEADER_SENT,
    "_headerSent",
    "5f68656164657253656e74"
);
runtime_atom!(HI_ATOM_U_CLOSED, "_closed", "5f636c6f736564");
runtime_atom!(HI_ATOM_U_HEADER, "_header", "5f686561646572");
runtime_atom!(
    HI_ATOM_U_KEEP_ALIVE_TIMEOUT,
    "_keepAliveTimeout",
    "5f6b656570416c69766554696d656f7574"
);
runtime_atom!(
    HI_ATOM_U_ON_PENDING_DATA,
    "_onPendingData",
    "5f6f6e50656e64696e6744617461"
);
runtime_atom!(HI_ATOM_REQ, "req", "726571");
runtime_atom!(HI_ATOM_U_SENT100, "_sent100", "5f73656e74313030");
runtime_atom!(
    HI_ATOM_U_EXPECT_CONTINUE,
    "_expect_continue",
    "5f6578706563745f636f6e74696e7565"
);
runtime_atom!(
    HI_ATOM_U_MAX_REQUESTS_PER_SOCKET,
    "_maxRequestsPerSocket",
    "5f6d61785265717565737473506572536f636b6574"
);

const OPT: u8 = crate::object::key_attrs::ENTRY_OPTIONAL;

/// `ServerResponse`'s slots past the emitter's three: `OutgoingMessage`'s
/// fields in node's constructor order, then its own.
pub(crate) mod response_slot {
    pub(crate) const OUTPUT_DATA: u32 = 3;
    pub(crate) const OUTPUT_SIZE: u32 = 4;
    pub(crate) const WRITABLE: u32 = 5;
    pub(crate) const DESTROYED: u32 = 6;
    pub(crate) const LAST: u32 = 7;
    pub(crate) const CHUNKED_ENCODING: u32 = 8;
    pub(crate) const SHOULD_KEEP_ALIVE: u32 = 9;
    pub(crate) const MAX_REQUESTS_ON_CONNECTION_REACHED: u32 = 10;
    pub(crate) const DEFAULT_KEEP_ALIVE: u32 = 11;
    pub(crate) const USE_CHUNKED_ENCODING_BY_DEFAULT: u32 = 12;
    pub(crate) const SEND_DATE: u32 = 13;
    pub(crate) const REMOVED_CONNECTION: u32 = 14;
    pub(crate) const REMOVED_CONT_LEN: u32 = 15;
    pub(crate) const REMOVED_TE: u32 = 16;
    pub(crate) const STRICT_CONTENT_LENGTH: u32 = 17;
    pub(crate) const CONTENT_LENGTH: u32 = 18;
    pub(crate) const HAS_BODY: u32 = 19;
    pub(crate) const TRAILER: u32 = 20;
    pub(crate) const FINISHED: u32 = 21;
    pub(crate) const HEADER_SENT: u32 = 22;
    pub(crate) const CLOSED: u32 = 23;
    pub(crate) const HEADER: u32 = 24;
    pub(crate) const KEEP_ALIVE_TIMEOUT: u32 = 25;
    pub(crate) const ON_PENDING_DATA: u32 = 26;
    pub(crate) const REQ: u32 = 27;
    pub(crate) const SENT100: u32 = 28;
    pub(crate) const EXPECT_CONTINUE: u32 = 29;
    pub(crate) const MAX_REQUESTS_PER_SOCKET: u32 = 30;
    pub(crate) const STATUS_MESSAGE: u32 = 31;
    pub(crate) const STATUS_CODE: u32 = 32;
    pub(crate) const PAYLOAD: u32 = 33;
}

/// The payload slot's entry (`native_payload::PAYLOAD_ENTRY`), optional:
/// the birth leaves it a hole and the payload attach fills it.
const PAYLOAD_SLOT: u8 = crate::object::key_attrs::PRIVATE_FIELD_ENTRY | OPT;

// `new ServerResponse(req)`: `OutgoingMessage.call(this)` (the emitter's
// init, then its 24 fields), then `ServerResponse`'s own.
runtime_layout!(SERVER_RESPONSE, 0x53490007, 0, &raw const SERVER_RESPONSE_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    HI_ATOM_OUTPUT_DATA @ response_slot::OUTPUT_DATA,
    HI_ATOM_OUTPUT_SIZE @ response_slot::OUTPUT_SIZE,
    NP_ATOM_WRITABLE @ response_slot::WRITABLE,
    NP_ATOM_DESTROYED @ response_slot::DESTROYED,
    HI_ATOM_U_LAST @ response_slot::LAST,
    HI_ATOM_CHUNKED_ENCODING @ response_slot::CHUNKED_ENCODING,
    HI_ATOM_SHOULD_KEEP_ALIVE @ response_slot::SHOULD_KEEP_ALIVE,
    HI_ATOM_MAX_REQUESTS_ON_CONNECTION_REACHED @ response_slot::MAX_REQUESTS_ON_CONNECTION_REACHED,
    HI_ATOM_U_DEFAULT_KEEP_ALIVE @ response_slot::DEFAULT_KEEP_ALIVE,
    HI_ATOM_USE_CHUNKED_ENCODING_BY_DEFAULT @ response_slot::USE_CHUNKED_ENCODING_BY_DEFAULT,
    HI_ATOM_SEND_DATE @ response_slot::SEND_DATE,
    HI_ATOM_U_REMOVED_CONNECTION @ response_slot::REMOVED_CONNECTION,
    HI_ATOM_U_REMOVED_CONT_LEN @ response_slot::REMOVED_CONT_LEN,
    HI_ATOM_U_REMOVED_TE @ response_slot::REMOVED_TE,
    HI_ATOM_STRICT_CONTENT_LENGTH @ response_slot::STRICT_CONTENT_LENGTH,
    HI_ATOM_U_CONTENT_LENGTH @ response_slot::CONTENT_LENGTH,
    HI_ATOM_U_HAS_BODY @ response_slot::HAS_BODY,
    HI_ATOM_U_TRAILER @ response_slot::TRAILER,
    HI_ATOM_FINISHED @ response_slot::FINISHED,
    HI_ATOM_U_HEADER_SENT @ response_slot::HEADER_SENT,
    HI_ATOM_U_CLOSED @ response_slot::CLOSED,
    HI_ATOM_U_HEADER @ response_slot::HEADER,
    HI_ATOM_U_KEEP_ALIVE_TIMEOUT @ response_slot::KEEP_ALIVE_TIMEOUT,
    HI_ATOM_U_ON_PENDING_DATA @ response_slot::ON_PENDING_DATA,
    HI_ATOM_REQ @ response_slot::REQ,
    HI_ATOM_U_SENT100 @ response_slot::SENT100,
    HI_ATOM_U_EXPECT_CONTINUE @ response_slot::EXPECT_CONTINUE,
    HI_ATOM_U_MAX_REQUESTS_PER_SOCKET @ response_slot::MAX_REQUESTS_PER_SOCKET => OPT,
    NP_ATOM_STATUS_MESSAGE @ response_slot::STATUS_MESSAGE => OPT,
    NP_ATOM_STATUS_CODE @ response_slot::STATUS_CODE => OPT,
    NP_SYMBOL_NATIVE_PAYLOAD @ response_slot::PAYLOAD => PAYLOAD_SLOT,
], realm = ["node:http.ServerResponse", "node:http2.Http2ServerResponse.prototype"]);

runtime_prototype_family!(
    INCOMING_MESSAGE_PROTO_FAMILY = INTRINSIC_SERIAL_INCOMING_MESSAGE_PROTOTYPE,
);

runtime_atom!(
    HI_ATOM_HTTP_VERSION_MAJOR,
    "httpVersionMajor",
    "6874747056657273696f6e4d616a6f72"
);
runtime_atom!(
    HI_ATOM_HTTP_VERSION_MINOR,
    "httpVersionMinor",
    "6874747056657273696f6e4d696e6f72"
);
runtime_atom!(HI_ATOM_HTTP_VERSION, "httpVersion", "6874747056657273696f6e");
runtime_atom!(HI_ATOM_COMPLETE, "complete", "636f6d706c657465");
runtime_atom!(HI_ATOM_RAW_HEADERS, "rawHeaders", "72617748656164657273");
runtime_atom!(HI_ATOM_RAW_TRAILERS, "rawTrailers", "726177547261696c657273");
runtime_atom!(
    HI_ATOM_JOIN_DUPLICATE_HEADERS,
    "joinDuplicateHeaders",
    "6a6f696e4475706c696361746548656164657273"
);
runtime_atom!(HI_ATOM_ABORTED, "aborted", "61626f72746564");
runtime_atom!(HI_ATOM_UPGRADE, "upgrade", "75706772616465");
runtime_atom!(reuse HI_ATOM_URL, "url", "75726c");
runtime_atom!(HI_ATOM_METHOD, "method", "6d6574686f64");
runtime_atom!(HI_ATOM_CLIENT, "client", "636c69656e74");
runtime_atom!(HI_ATOM_U_CONSUMING, "_consuming", "5f636f6e73756d696e67");
runtime_atom!(HI_ATOM_U_DUMPED, "_dumped", "5f64756d706564");

/// `IncomingMessage`'s slots: Readable's (the emitter's three, the state,
/// the optional hooks), then node's constructor fields in its order, then
/// its symbol-keyed fields.
pub(crate) mod incoming_slot {
    pub(crate) const SOCKET: u32 = 7;
    pub(crate) const HTTP_VERSION_MAJOR: u32 = 8;
    pub(crate) const HTTP_VERSION_MINOR: u32 = 9;
    pub(crate) const HTTP_VERSION: u32 = 10;
    pub(crate) const COMPLETE: u32 = 11;
    pub(crate) const RAW_HEADERS: u32 = 12;
    pub(crate) const RAW_TRAILERS: u32 = 13;
    pub(crate) const JOIN_DUPLICATE_HEADERS: u32 = 14;
    pub(crate) const ABORTED: u32 = 15;
    pub(crate) const UPGRADE: u32 = 16;
    pub(crate) const URL: u32 = 17;
    pub(crate) const METHOD: u32 = 18;
    pub(crate) const STATUS_CODE: u32 = 19;
    pub(crate) const STATUS_MESSAGE: u32 = 20;
    pub(crate) const CLIENT: u32 = 21;
    pub(crate) const CONSUMING: u32 = 22;
    pub(crate) const DUMPED: u32 = 23;
    pub(crate) const K_HEADERS: u32 = 24;
    pub(crate) const K_HEADERS_COUNT: u32 = 25;
    pub(crate) const K_TRAILERS: u32 = 26;
    pub(crate) const K_TRAILERS_COUNT: u32 = 27;
    pub(crate) const K_ABORT_CONTROLLER: u32 = 28;
    pub(crate) const K_ABORT_SIGNAL_SOCKET: u32 = 29;
    pub(crate) const K_ABORT_SIGNAL_LISTENER: u32 = 30;
    pub(crate) const K_ABORT_SIGNAL_DETACHED: u32 = 31;
    pub(crate) const PAYLOAD: u32 = 32;
}

// `new IncomingMessage(socket)`: `Readable.call(this, opts)` (READABLE's
// keys, but `_eventsCount`, which the first listener creates after every
// constructor field), then the constructor's fields. The payload key comes
// first: a server request's payload is attached before the constructor
// runs (the stream's state record joins the attached cell), and a key that
// is present ahead of the constructor's holes leaves each of their fills in
// place. It is a private entry, so no enumeration sees its position.
runtime_layout!(INCOMING_MESSAGE, 0x53490009, 0, &raw const INCOMING_MESSAGE_PROTO_FAMILY, [
    NP_SYMBOL_NATIVE_PAYLOAD @ incoming_slot::PAYLOAD => PAYLOAD_SLOT,
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_READABLE_STATE @ readable_slot::STATE,
    NP_ATOM_U_READ @ readable_slot::READ => OPT,
    NP_ATOM_U_DESTROY @ readable_slot::DESTROY => OPT,
    SI_ATOM_U_CONSTRUCT @ readable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_SOCKET @ incoming_slot::SOCKET,
    HI_ATOM_HTTP_VERSION_MAJOR @ incoming_slot::HTTP_VERSION_MAJOR,
    HI_ATOM_HTTP_VERSION_MINOR @ incoming_slot::HTTP_VERSION_MINOR,
    HI_ATOM_HTTP_VERSION @ incoming_slot::HTTP_VERSION,
    HI_ATOM_COMPLETE @ incoming_slot::COMPLETE,
    HI_ATOM_RAW_HEADERS @ incoming_slot::RAW_HEADERS,
    HI_ATOM_RAW_TRAILERS @ incoming_slot::RAW_TRAILERS,
    HI_ATOM_JOIN_DUPLICATE_HEADERS @ incoming_slot::JOIN_DUPLICATE_HEADERS,
    HI_ATOM_ABORTED @ incoming_slot::ABORTED,
    HI_ATOM_UPGRADE @ incoming_slot::UPGRADE,
    HI_ATOM_URL @ incoming_slot::URL,
    HI_ATOM_METHOD @ incoming_slot::METHOD,
    NP_ATOM_STATUS_CODE @ incoming_slot::STATUS_CODE,
    NP_ATOM_STATUS_MESSAGE @ incoming_slot::STATUS_MESSAGE,
    HI_ATOM_CLIENT @ incoming_slot::CLIENT,
    HI_ATOM_U_CONSUMING @ incoming_slot::CONSUMING,
    HI_ATOM_U_DUMPED @ incoming_slot::DUMPED,
    NP_SYMBOL_K_HEADERS @ incoming_slot::K_HEADERS,
    NP_SYMBOL_K_HEADERS_COUNT @ incoming_slot::K_HEADERS_COUNT,
    NP_SYMBOL_K_TRAILERS @ incoming_slot::K_TRAILERS,
    NP_SYMBOL_K_TRAILERS_COUNT @ incoming_slot::K_TRAILERS_COUNT,
    NP_SYMBOL_K_ABORT_CONTROLLER @ incoming_slot::K_ABORT_CONTROLLER,
    NP_SYMBOL_K_ABORT_SIGNAL_SOCKET @ incoming_slot::K_ABORT_SIGNAL_SOCKET,
    NP_SYMBOL_K_ABORT_SIGNAL_LISTENER @ incoming_slot::K_ABORT_SIGNAL_LISTENER,
    NP_SYMBOL_K_ABORT_SIGNAL_DETACHED @ incoming_slot::K_ABORT_SIGNAL_DETACHED,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:http.IncomingMessage", "node:http.IncomingMessage.prototype"]);

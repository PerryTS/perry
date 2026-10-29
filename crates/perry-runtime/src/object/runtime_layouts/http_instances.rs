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
}

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
], realm = ["node:http.ServerResponse"]);

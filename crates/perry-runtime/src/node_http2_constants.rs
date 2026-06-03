//! `node:http2` module-level constants and symbols.

use crate::value::JSValue;
use std::cell::Cell;

thread_local! {
    static SENSITIVE_HEADERS_SYMBOL: Cell<u64> = const { Cell::new(0) };
}

pub(crate) const HTTP2_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Http2ServerRequest",
    b"Http2ServerResponse",
    b"connect",
    b"constants",
    b"createSecureServer",
    b"createServer",
    b"getDefaultSettings",
    b"getPackedSettings",
    b"getUnpackedSettings",
    b"performServerHandshake",
    b"sensitiveHeaders",
];

macro_rules! with_http2_constants {
    ($callback:ident $(, $arg:expr)?) => {
        $callback! {
            $($arg,)?
            NGHTTP2_ERR_FRAME_SIZE_ERROR => Http2ConstantValue::Number(-522.0),
            NGHTTP2_SESSION_SERVER => Http2ConstantValue::Number(0.0),
            NGHTTP2_SESSION_CLIENT => Http2ConstantValue::Number(1.0),
            NGHTTP2_STREAM_STATE_IDLE => Http2ConstantValue::Number(1.0),
            NGHTTP2_STREAM_STATE_OPEN => Http2ConstantValue::Number(2.0),
            NGHTTP2_STREAM_STATE_RESERVED_LOCAL => Http2ConstantValue::Number(3.0),
            NGHTTP2_STREAM_STATE_RESERVED_REMOTE => Http2ConstantValue::Number(4.0),
            NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL => Http2ConstantValue::Number(5.0),
            NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE => Http2ConstantValue::Number(6.0),
            NGHTTP2_STREAM_STATE_CLOSED => Http2ConstantValue::Number(7.0),
            NGHTTP2_FLAG_NONE => Http2ConstantValue::Number(0.0),
            NGHTTP2_FLAG_END_STREAM => Http2ConstantValue::Number(1.0),
            NGHTTP2_FLAG_END_HEADERS => Http2ConstantValue::Number(4.0),
            NGHTTP2_FLAG_ACK => Http2ConstantValue::Number(1.0),
            NGHTTP2_FLAG_PADDED => Http2ConstantValue::Number(8.0),
            NGHTTP2_FLAG_PRIORITY => Http2ConstantValue::Number(32.0),
            DEFAULT_SETTINGS_HEADER_TABLE_SIZE => Http2ConstantValue::Number(4096.0),
            DEFAULT_SETTINGS_ENABLE_PUSH => Http2ConstantValue::Number(1.0),
            DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS => Http2ConstantValue::Number(4294967295.0),
            DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE => Http2ConstantValue::Number(65535.0),
            DEFAULT_SETTINGS_MAX_FRAME_SIZE => Http2ConstantValue::Number(16384.0),
            DEFAULT_SETTINGS_MAX_HEADER_LIST_SIZE => Http2ConstantValue::Number(65535.0),
            DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL => Http2ConstantValue::Number(0.0),
            MAX_MAX_FRAME_SIZE => Http2ConstantValue::Number(16777215.0),
            MIN_MAX_FRAME_SIZE => Http2ConstantValue::Number(16384.0),
            MAX_INITIAL_WINDOW_SIZE => Http2ConstantValue::Number(2147483647.0),
            NGHTTP2_SETTINGS_HEADER_TABLE_SIZE => Http2ConstantValue::Number(1.0),
            NGHTTP2_SETTINGS_ENABLE_PUSH => Http2ConstantValue::Number(2.0),
            NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS => Http2ConstantValue::Number(3.0),
            NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE => Http2ConstantValue::Number(4.0),
            NGHTTP2_SETTINGS_MAX_FRAME_SIZE => Http2ConstantValue::Number(5.0),
            NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE => Http2ConstantValue::Number(6.0),
            NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL => Http2ConstantValue::Number(8.0),
            PADDING_STRATEGY_NONE => Http2ConstantValue::Number(0.0),
            PADDING_STRATEGY_ALIGNED => Http2ConstantValue::Number(1.0),
            PADDING_STRATEGY_MAX => Http2ConstantValue::Number(2.0),
            PADDING_STRATEGY_CALLBACK => Http2ConstantValue::Number(1.0),
            NGHTTP2_NO_ERROR => Http2ConstantValue::Number(0.0),
            NGHTTP2_PROTOCOL_ERROR => Http2ConstantValue::Number(1.0),
            NGHTTP2_INTERNAL_ERROR => Http2ConstantValue::Number(2.0),
            NGHTTP2_FLOW_CONTROL_ERROR => Http2ConstantValue::Number(3.0),
            NGHTTP2_SETTINGS_TIMEOUT => Http2ConstantValue::Number(4.0),
            NGHTTP2_STREAM_CLOSED => Http2ConstantValue::Number(5.0),
            NGHTTP2_FRAME_SIZE_ERROR => Http2ConstantValue::Number(6.0),
            NGHTTP2_REFUSED_STREAM => Http2ConstantValue::Number(7.0),
            NGHTTP2_CANCEL => Http2ConstantValue::Number(8.0),
            NGHTTP2_COMPRESSION_ERROR => Http2ConstantValue::Number(9.0),
            NGHTTP2_CONNECT_ERROR => Http2ConstantValue::Number(10.0),
            NGHTTP2_ENHANCE_YOUR_CALM => Http2ConstantValue::Number(11.0),
            NGHTTP2_INADEQUATE_SECURITY => Http2ConstantValue::Number(12.0),
            NGHTTP2_HTTP_1_1_REQUIRED => Http2ConstantValue::Number(13.0),
            NGHTTP2_DEFAULT_WEIGHT => Http2ConstantValue::Number(16.0),
            HTTP2_HEADER_STATUS => Http2ConstantValue::String(":status"),
            HTTP2_HEADER_METHOD => Http2ConstantValue::String(":method"),
            HTTP2_HEADER_AUTHORITY => Http2ConstantValue::String(":authority"),
            HTTP2_HEADER_SCHEME => Http2ConstantValue::String(":scheme"),
            HTTP2_HEADER_PATH => Http2ConstantValue::String(":path"),
            HTTP2_HEADER_PROTOCOL => Http2ConstantValue::String(":protocol"),
            HTTP2_HEADER_ACCEPT_ENCODING => Http2ConstantValue::String("accept-encoding"),
            HTTP2_HEADER_ACCEPT_LANGUAGE => Http2ConstantValue::String("accept-language"),
            HTTP2_HEADER_ACCEPT_RANGES => Http2ConstantValue::String("accept-ranges"),
            HTTP2_HEADER_ACCEPT => Http2ConstantValue::String("accept"),
            HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS => Http2ConstantValue::String("access-control-allow-credentials"),
            HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS => Http2ConstantValue::String("access-control-allow-headers"),
            HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS => Http2ConstantValue::String("access-control-allow-methods"),
            HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN => Http2ConstantValue::String("access-control-allow-origin"),
            HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS => Http2ConstantValue::String("access-control-expose-headers"),
            HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS => Http2ConstantValue::String("access-control-request-headers"),
            HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD => Http2ConstantValue::String("access-control-request-method"),
            HTTP2_HEADER_AGE => Http2ConstantValue::String("age"),
            HTTP2_HEADER_AUTHORIZATION => Http2ConstantValue::String("authorization"),
            HTTP2_HEADER_CACHE_CONTROL => Http2ConstantValue::String("cache-control"),
            HTTP2_HEADER_CONNECTION => Http2ConstantValue::String("connection"),
            HTTP2_HEADER_CONTENT_DISPOSITION => Http2ConstantValue::String("content-disposition"),
            HTTP2_HEADER_CONTENT_ENCODING => Http2ConstantValue::String("content-encoding"),
            HTTP2_HEADER_CONTENT_LENGTH => Http2ConstantValue::String("content-length"),
            HTTP2_HEADER_CONTENT_TYPE => Http2ConstantValue::String("content-type"),
            HTTP2_HEADER_COOKIE => Http2ConstantValue::String("cookie"),
            HTTP2_HEADER_DATE => Http2ConstantValue::String("date"),
            HTTP2_HEADER_ETAG => Http2ConstantValue::String("etag"),
            HTTP2_HEADER_FORWARDED => Http2ConstantValue::String("forwarded"),
            HTTP2_HEADER_HOST => Http2ConstantValue::String("host"),
            HTTP2_HEADER_IF_MODIFIED_SINCE => Http2ConstantValue::String("if-modified-since"),
            HTTP2_HEADER_IF_NONE_MATCH => Http2ConstantValue::String("if-none-match"),
            HTTP2_HEADER_IF_RANGE => Http2ConstantValue::String("if-range"),
            HTTP2_HEADER_LAST_MODIFIED => Http2ConstantValue::String("last-modified"),
            HTTP2_HEADER_LINK => Http2ConstantValue::String("link"),
            HTTP2_HEADER_LOCATION => Http2ConstantValue::String("location"),
            HTTP2_HEADER_RANGE => Http2ConstantValue::String("range"),
            HTTP2_HEADER_REFERER => Http2ConstantValue::String("referer"),
            HTTP2_HEADER_SERVER => Http2ConstantValue::String("server"),
            HTTP2_HEADER_SET_COOKIE => Http2ConstantValue::String("set-cookie"),
            HTTP2_HEADER_STRICT_TRANSPORT_SECURITY => Http2ConstantValue::String("strict-transport-security"),
            HTTP2_HEADER_TRANSFER_ENCODING => Http2ConstantValue::String("transfer-encoding"),
            HTTP2_HEADER_TE => Http2ConstantValue::String("te"),
            HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS => Http2ConstantValue::String("upgrade-insecure-requests"),
            HTTP2_HEADER_UPGRADE => Http2ConstantValue::String("upgrade"),
            HTTP2_HEADER_USER_AGENT => Http2ConstantValue::String("user-agent"),
            HTTP2_HEADER_VARY => Http2ConstantValue::String("vary"),
            HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS => Http2ConstantValue::String("x-content-type-options"),
            HTTP2_HEADER_X_FRAME_OPTIONS => Http2ConstantValue::String("x-frame-options"),
            HTTP2_HEADER_KEEP_ALIVE => Http2ConstantValue::String("keep-alive"),
            HTTP2_HEADER_PROXY_CONNECTION => Http2ConstantValue::String("proxy-connection"),
            HTTP2_HEADER_X_XSS_PROTECTION => Http2ConstantValue::String("x-xss-protection"),
            HTTP2_HEADER_ALT_SVC => Http2ConstantValue::String("alt-svc"),
            HTTP2_HEADER_CONTENT_SECURITY_POLICY => Http2ConstantValue::String("content-security-policy"),
            HTTP2_HEADER_EARLY_DATA => Http2ConstantValue::String("early-data"),
            HTTP2_HEADER_EXPECT_CT => Http2ConstantValue::String("expect-ct"),
            HTTP2_HEADER_ORIGIN => Http2ConstantValue::String("origin"),
            HTTP2_HEADER_PURPOSE => Http2ConstantValue::String("purpose"),
            HTTP2_HEADER_TIMING_ALLOW_ORIGIN => Http2ConstantValue::String("timing-allow-origin"),
            HTTP2_HEADER_X_FORWARDED_FOR => Http2ConstantValue::String("x-forwarded-for"),
            HTTP2_HEADER_PRIORITY => Http2ConstantValue::String("priority"),
            HTTP2_HEADER_ACCEPT_CHARSET => Http2ConstantValue::String("accept-charset"),
            HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE => Http2ConstantValue::String("access-control-max-age"),
            HTTP2_HEADER_ALLOW => Http2ConstantValue::String("allow"),
            HTTP2_HEADER_CONTENT_LANGUAGE => Http2ConstantValue::String("content-language"),
            HTTP2_HEADER_CONTENT_LOCATION => Http2ConstantValue::String("content-location"),
            HTTP2_HEADER_CONTENT_MD5 => Http2ConstantValue::String("content-md5"),
            HTTP2_HEADER_CONTENT_RANGE => Http2ConstantValue::String("content-range"),
            HTTP2_HEADER_DNT => Http2ConstantValue::String("dnt"),
            HTTP2_HEADER_EXPECT => Http2ConstantValue::String("expect"),
            HTTP2_HEADER_EXPIRES => Http2ConstantValue::String("expires"),
            HTTP2_HEADER_FROM => Http2ConstantValue::String("from"),
            HTTP2_HEADER_IF_MATCH => Http2ConstantValue::String("if-match"),
            HTTP2_HEADER_IF_UNMODIFIED_SINCE => Http2ConstantValue::String("if-unmodified-since"),
            HTTP2_HEADER_MAX_FORWARDS => Http2ConstantValue::String("max-forwards"),
            HTTP2_HEADER_PREFER => Http2ConstantValue::String("prefer"),
            HTTP2_HEADER_PROXY_AUTHENTICATE => Http2ConstantValue::String("proxy-authenticate"),
            HTTP2_HEADER_PROXY_AUTHORIZATION => Http2ConstantValue::String("proxy-authorization"),
            HTTP2_HEADER_REFRESH => Http2ConstantValue::String("refresh"),
            HTTP2_HEADER_RETRY_AFTER => Http2ConstantValue::String("retry-after"),
            HTTP2_HEADER_TRAILER => Http2ConstantValue::String("trailer"),
            HTTP2_HEADER_TK => Http2ConstantValue::String("tk"),
            HTTP2_HEADER_VIA => Http2ConstantValue::String("via"),
            HTTP2_HEADER_WARNING => Http2ConstantValue::String("warning"),
            HTTP2_HEADER_WWW_AUTHENTICATE => Http2ConstantValue::String("www-authenticate"),
            HTTP2_HEADER_HTTP2_SETTINGS => Http2ConstantValue::String("http2-settings"),
            HTTP2_METHOD_ACL => Http2ConstantValue::String("ACL"),
            HTTP2_METHOD_BASELINE_CONTROL => Http2ConstantValue::String("BASELINE-CONTROL"),
            HTTP2_METHOD_BIND => Http2ConstantValue::String("BIND"),
            HTTP2_METHOD_CHECKIN => Http2ConstantValue::String("CHECKIN"),
            HTTP2_METHOD_CHECKOUT => Http2ConstantValue::String("CHECKOUT"),
            HTTP2_METHOD_CONNECT => Http2ConstantValue::String("CONNECT"),
            HTTP2_METHOD_COPY => Http2ConstantValue::String("COPY"),
            HTTP2_METHOD_DELETE => Http2ConstantValue::String("DELETE"),
            HTTP2_METHOD_GET => Http2ConstantValue::String("GET"),
            HTTP2_METHOD_HEAD => Http2ConstantValue::String("HEAD"),
            HTTP2_METHOD_LABEL => Http2ConstantValue::String("LABEL"),
            HTTP2_METHOD_LINK => Http2ConstantValue::String("LINK"),
            HTTP2_METHOD_LOCK => Http2ConstantValue::String("LOCK"),
            HTTP2_METHOD_MERGE => Http2ConstantValue::String("MERGE"),
            HTTP2_METHOD_MKACTIVITY => Http2ConstantValue::String("MKACTIVITY"),
            HTTP2_METHOD_MKCALENDAR => Http2ConstantValue::String("MKCALENDAR"),
            HTTP2_METHOD_MKCOL => Http2ConstantValue::String("MKCOL"),
            HTTP2_METHOD_MKREDIRECTREF => Http2ConstantValue::String("MKREDIRECTREF"),
            HTTP2_METHOD_MKWORKSPACE => Http2ConstantValue::String("MKWORKSPACE"),
            HTTP2_METHOD_MOVE => Http2ConstantValue::String("MOVE"),
            HTTP2_METHOD_OPTIONS => Http2ConstantValue::String("OPTIONS"),
            HTTP2_METHOD_ORDERPATCH => Http2ConstantValue::String("ORDERPATCH"),
            HTTP2_METHOD_PATCH => Http2ConstantValue::String("PATCH"),
            HTTP2_METHOD_POST => Http2ConstantValue::String("POST"),
            HTTP2_METHOD_PRI => Http2ConstantValue::String("PRI"),
            HTTP2_METHOD_PROPFIND => Http2ConstantValue::String("PROPFIND"),
            HTTP2_METHOD_PROPPATCH => Http2ConstantValue::String("PROPPATCH"),
            HTTP2_METHOD_PUT => Http2ConstantValue::String("PUT"),
            HTTP2_METHOD_REBIND => Http2ConstantValue::String("REBIND"),
            HTTP2_METHOD_REPORT => Http2ConstantValue::String("REPORT"),
            HTTP2_METHOD_SEARCH => Http2ConstantValue::String("SEARCH"),
            HTTP2_METHOD_TRACE => Http2ConstantValue::String("TRACE"),
            HTTP2_METHOD_UNBIND => Http2ConstantValue::String("UNBIND"),
            HTTP2_METHOD_UNCHECKOUT => Http2ConstantValue::String("UNCHECKOUT"),
            HTTP2_METHOD_UNLINK => Http2ConstantValue::String("UNLINK"),
            HTTP2_METHOD_UNLOCK => Http2ConstantValue::String("UNLOCK"),
            HTTP2_METHOD_UPDATE => Http2ConstantValue::String("UPDATE"),
            HTTP2_METHOD_UPDATEREDIRECTREF => Http2ConstantValue::String("UPDATEREDIRECTREF"),
            HTTP2_METHOD_VERSION_CONTROL => Http2ConstantValue::String("VERSION-CONTROL"),
            HTTP_STATUS_CONTINUE => Http2ConstantValue::Number(100.0),
            HTTP_STATUS_SWITCHING_PROTOCOLS => Http2ConstantValue::Number(101.0),
            HTTP_STATUS_PROCESSING => Http2ConstantValue::Number(102.0),
            HTTP_STATUS_EARLY_HINTS => Http2ConstantValue::Number(103.0),
            HTTP_STATUS_OK => Http2ConstantValue::Number(200.0),
            HTTP_STATUS_CREATED => Http2ConstantValue::Number(201.0),
            HTTP_STATUS_ACCEPTED => Http2ConstantValue::Number(202.0),
            HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION => Http2ConstantValue::Number(203.0),
            HTTP_STATUS_NO_CONTENT => Http2ConstantValue::Number(204.0),
            HTTP_STATUS_RESET_CONTENT => Http2ConstantValue::Number(205.0),
            HTTP_STATUS_PARTIAL_CONTENT => Http2ConstantValue::Number(206.0),
            HTTP_STATUS_MULTI_STATUS => Http2ConstantValue::Number(207.0),
            HTTP_STATUS_ALREADY_REPORTED => Http2ConstantValue::Number(208.0),
            HTTP_STATUS_IM_USED => Http2ConstantValue::Number(226.0),
            HTTP_STATUS_MULTIPLE_CHOICES => Http2ConstantValue::Number(300.0),
            HTTP_STATUS_MOVED_PERMANENTLY => Http2ConstantValue::Number(301.0),
            HTTP_STATUS_FOUND => Http2ConstantValue::Number(302.0),
            HTTP_STATUS_SEE_OTHER => Http2ConstantValue::Number(303.0),
            HTTP_STATUS_NOT_MODIFIED => Http2ConstantValue::Number(304.0),
            HTTP_STATUS_USE_PROXY => Http2ConstantValue::Number(305.0),
            HTTP_STATUS_TEMPORARY_REDIRECT => Http2ConstantValue::Number(307.0),
            HTTP_STATUS_PERMANENT_REDIRECT => Http2ConstantValue::Number(308.0),
            HTTP_STATUS_BAD_REQUEST => Http2ConstantValue::Number(400.0),
            HTTP_STATUS_UNAUTHORIZED => Http2ConstantValue::Number(401.0),
            HTTP_STATUS_PAYMENT_REQUIRED => Http2ConstantValue::Number(402.0),
            HTTP_STATUS_FORBIDDEN => Http2ConstantValue::Number(403.0),
            HTTP_STATUS_NOT_FOUND => Http2ConstantValue::Number(404.0),
            HTTP_STATUS_METHOD_NOT_ALLOWED => Http2ConstantValue::Number(405.0),
            HTTP_STATUS_NOT_ACCEPTABLE => Http2ConstantValue::Number(406.0),
            HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED => Http2ConstantValue::Number(407.0),
            HTTP_STATUS_REQUEST_TIMEOUT => Http2ConstantValue::Number(408.0),
            HTTP_STATUS_CONFLICT => Http2ConstantValue::Number(409.0),
            HTTP_STATUS_GONE => Http2ConstantValue::Number(410.0),
            HTTP_STATUS_LENGTH_REQUIRED => Http2ConstantValue::Number(411.0),
            HTTP_STATUS_PRECONDITION_FAILED => Http2ConstantValue::Number(412.0),
            HTTP_STATUS_PAYLOAD_TOO_LARGE => Http2ConstantValue::Number(413.0),
            HTTP_STATUS_URI_TOO_LONG => Http2ConstantValue::Number(414.0),
            HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE => Http2ConstantValue::Number(415.0),
            HTTP_STATUS_RANGE_NOT_SATISFIABLE => Http2ConstantValue::Number(416.0),
            HTTP_STATUS_EXPECTATION_FAILED => Http2ConstantValue::Number(417.0),
            HTTP_STATUS_TEAPOT => Http2ConstantValue::Number(418.0),
            HTTP_STATUS_MISDIRECTED_REQUEST => Http2ConstantValue::Number(421.0),
            HTTP_STATUS_UNPROCESSABLE_ENTITY => Http2ConstantValue::Number(422.0),
            HTTP_STATUS_LOCKED => Http2ConstantValue::Number(423.0),
            HTTP_STATUS_FAILED_DEPENDENCY => Http2ConstantValue::Number(424.0),
            HTTP_STATUS_TOO_EARLY => Http2ConstantValue::Number(425.0),
            HTTP_STATUS_UPGRADE_REQUIRED => Http2ConstantValue::Number(426.0),
            HTTP_STATUS_PRECONDITION_REQUIRED => Http2ConstantValue::Number(428.0),
            HTTP_STATUS_TOO_MANY_REQUESTS => Http2ConstantValue::Number(429.0),
            HTTP_STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE => Http2ConstantValue::Number(431.0),
            HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS => Http2ConstantValue::Number(451.0),
            HTTP_STATUS_INTERNAL_SERVER_ERROR => Http2ConstantValue::Number(500.0),
            HTTP_STATUS_NOT_IMPLEMENTED => Http2ConstantValue::Number(501.0),
            HTTP_STATUS_BAD_GATEWAY => Http2ConstantValue::Number(502.0),
            HTTP_STATUS_SERVICE_UNAVAILABLE => Http2ConstantValue::Number(503.0),
            HTTP_STATUS_GATEWAY_TIMEOUT => Http2ConstantValue::Number(504.0),
            HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED => Http2ConstantValue::Number(505.0),
            HTTP_STATUS_VARIANT_ALSO_NEGOTIATES => Http2ConstantValue::Number(506.0),
            HTTP_STATUS_INSUFFICIENT_STORAGE => Http2ConstantValue::Number(507.0),
            HTTP_STATUS_LOOP_DETECTED => Http2ConstantValue::Number(508.0),
            HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED => Http2ConstantValue::Number(509.0),
            HTTP_STATUS_NOT_EXTENDED => Http2ConstantValue::Number(510.0),
            HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED => Http2ConstantValue::Number(511.0),
        }
    };
}

macro_rules! http2_constant_keys {
    ($($name:ident => $value:expr,)*) => {
        &[$(stringify!($name).as_bytes()),*]
    };
}

macro_rules! http2_constant_match {
    ($prop:expr, $($name:ident => $value:expr,)*) => {
        match $prop {
            $(stringify!($name) => Some(materialize($value)),)*
            _ => None,
        }
    };
}

enum Http2ConstantValue {
    Number(f64),
    String(&'static str),
}

pub(crate) const HTTP2_CONSTANTS_KEYS: &[&[u8]] = with_http2_constants!(http2_constant_keys);

pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SENSITIVE_HEADERS_SYMBOL.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
}

pub(crate) fn sensitive_headers_symbol() -> f64 {
    SENSITIVE_HEADERS_SYMBOL.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let value = unsafe { crate::symbol::js_symbol_new(string_value("sensitiveHeaders")) };
        slot.set(value.to_bits());
        value
    })
}

pub(crate) fn constant(prop: &str) -> Option<f64> {
    with_http2_constants!(http2_constant_match, prop)
}

fn materialize(value: Http2ConstantValue) -> f64 {
    match value {
        Http2ConstantValue::Number(value) => value,
        Http2ConstantValue::String(value) => string_value(value),
    }
}

fn string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

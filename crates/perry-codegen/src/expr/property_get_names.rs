pub(super) fn is_headers_method_name(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "delete"
            | "entries"
            | "forEach"
            | "get"
            | "getSetCookie"
            | "has"
            | "keys"
            | "set"
            | "values"
    )
}

pub(super) fn is_http_client_request_method_name(name: &str) -> bool {
    matches!(
        name,
        "on" | "end"
            | "write"
            | "setHeader"
            | "setTimeout"
            | "listenerCount"
            | "getHeader"
            | "hasHeader"
            | "removeHeader"
            | "getHeaderNames"
            | "getHeaders"
            | "getRawHeaderNames"
            | "abort"
            | "destroy"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setNoDelay"
            | "setSocketKeepAlive"
    )
}

pub(super) fn is_url_pattern_data_property(name: &str) -> bool {
    matches!(
        name,
        "protocol"
            | "username"
            | "password"
            | "hostname"
            | "port"
            | "pathname"
            | "search"
            | "hash"
            | "hasRegExpGroups"
    )
}

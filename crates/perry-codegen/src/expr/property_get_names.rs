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

fn is_net_socket_method_name(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "connect"
            | "destroy"
            | "destroySoon"
            | "end"
            | "pause"
            | "ref"
            | "resetAndDestroy"
            | "resume"
            | "setEncoding"
            | "setKeepAlive"
            | "setNoDelay"
            | "getTypeOfService"
            | "setTypeOfService"
            | "setTimeout"
            | "unref"
            | "write"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
            | "upgradeToTLS"
            | "setDefaultEncoding"
            | "cork"
            | "uncork"
    )
}

fn is_net_server_method_name(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "close"
            | "getConnections"
            | "listen"
            | "ref"
            | "unref"
            | "setTimeout"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
    )
}

fn is_net_block_list_method_name(name: &str) -> bool {
    matches!(
        name,
        "addAddress" | "addRange" | "addSubnet" | "check" | "toJSON" | "fromJSON"
    )
}

pub(super) fn is_net_native_method_value(class_name: &str, name: &str) -> bool {
    match class_name {
        "Socket" => is_net_socket_method_name(name),
        "Server" => is_net_server_method_name(name),
        "BlockList" => is_net_block_list_method_name(name),
        _ => false,
    }
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

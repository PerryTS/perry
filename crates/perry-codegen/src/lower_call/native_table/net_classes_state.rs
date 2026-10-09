use super::*;

pub(super) const NET_CLASSES_STATE_ROWS: &[NativeModSig] = &[
    // #11227 — a typed `net.Server` had no `prependListener` /
    // `prependOnceListener` row (the Socket rows live in `net_events.rs`,
    // which is at the file-size cap), so the call registered nothing.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "BlockList",
        class_filter: None,
        runtime: "js_net_block_list_new",
        args: &[],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "SocketAddress",
        class_filter: None,
        runtime: "js_net_socket_address_new",
        args: &[NA_F64],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isBlockList",
        class_filter: Some("BlockList"),
        runtime: "js_net_block_list_is_block_list",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "parse",
        class_filter: Some("SocketAddress"),
        runtime: "js_net_socket_address_parse_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
];

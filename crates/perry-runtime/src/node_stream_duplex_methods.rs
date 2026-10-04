//! node:stream — Duplex prototype method table. Split out of
//! node_stream_readwrite.rs for the 2000-line file-size gate.
use super::*;

pub(super) fn duplex_methods() -> [(&'static str, StubFn); 44] {
    // Union of readable + writable, deduped (`on/once/off/addListener/
    // removeListener/removeAllListeners/emit/listenerCount/listeners/
    // destroy` appear once each).
    [
        ("on", crate::fn_info!(ns_on2, 2; with_declared(2))),
        ("once", crate::fn_info!(ns_once2, 2; with_declared(2))),
        (
            "prependListener",
            crate::fn_info!(ns_prepend_listener2, 2; with_declared(2)),
        ),
        (
            "prependOnceListener",
            crate::fn_info!(ns_prepend_once_listener2, 2; with_declared(2)),
        ),
        ("off", crate::fn_info!(ns_off2, 2; with_declared(2))),
        ("addListener", crate::fn_info!(ns_on2, 2; with_declared(2))),
        (
            "removeListener",
            crate::fn_info!(ns_remove_listener2, 2; with_declared(2)),
        ),
        (
            "removeAllListeners",
            crate::fn_info!(ns_remove_all_listeners1, 1; with_declared(1)),
        ),
        ("emit", crate::fn_info!(ns_emit_rest, 2; with_rest(1))),
        (
            "setMaxListeners",
            crate::fn_info!(ns_set_max_listeners, 1; with_declared(1)),
        ),
        (
            "getMaxListeners",
            crate::fn_info!(ns_get_max_listeners, 0; with_declared(0)),
        ),
        (
            "eventNames",
            crate::fn_info!(ns_event_names, 0; with_declared(0)),
        ),
        (
            "listenerCount",
            crate::fn_info!(ns_listener_count, 2; with_declared(2)),
        ),
        (
            "listeners",
            crate::fn_info!(ns_listeners, 1; with_declared(1)),
        ),
        (
            "rawListeners",
            crate::fn_info!(ns_raw_listeners, 1; with_declared(1)),
        ),
        ("read", crate::fn_info!(ns_read1, 1; with_declared(1))),
        ("pipe", crate::fn_info!(ns_pipe2, 2; with_declared(2))),
        ("unpipe", crate::fn_info!(ns_unpipe1, 1; with_declared(1))),
        ("wrap", crate::fn_info!(ns_wrap1, 1; with_declared(1))),
        ("pause", crate::fn_info!(ns_pause0, 0; with_declared(0))),
        ("resume", crate::fn_info!(ns_resume0, 0; with_declared(0))),
        ("setEncoding", crate::fn_info!(ns_set_encoding1, 1)),
        (
            "isPaused",
            crate::fn_info!(ns_is_paused0, 0; with_declared(0)),
        ),
        (
            "toArray",
            crate::fn_info!(ns_iter_to_array, 1; with_declared(1)),
        ),
        ("map", crate::fn_info!(ns_iter_map, 2; with_declared(2))),
        (
            "filter",
            crate::fn_info!(ns_iter_filter, 2; with_declared(2)),
        ),
        (
            "reduce",
            crate::fn_info!(ns_iter_reduce, 3; with_declared(3)),
        ),
        (
            "forEach",
            crate::fn_info!(ns_iter_for_each, 2; with_declared(2)),
        ),
        ("find", crate::fn_info!(ns_iter_find, 2; with_declared(2))),
        ("some", crate::fn_info!(ns_iter_some, 2; with_declared(2))),
        ("every", crate::fn_info!(ns_iter_every, 2; with_declared(2))),
        (
            "flatMap",
            crate::fn_info!(ns_iter_flat_map, 2; with_declared(2)),
        ),
        ("take", crate::fn_info!(ns_iter_take, 1; with_declared(1))),
        ("drop", crate::fn_info!(ns_iter_drop, 1; with_declared(1))),
        ("iterator", crate::fn_info!(async_iterator::ns_iterator1, 1)),
        ("push", crate::fn_info!(ns_push1, 1; with_declared(1))),
        ("unshift", crate::fn_info!(ns_unshift1, 1; with_declared(1))),
        ("compose", crate::fn_info!(ns_compose1, 1; with_declared(1))),
        ("write", crate::fn_info!(ns_write3, 3; with_declared(3))),
        ("end", crate::fn_info!(ns_end3, 3; with_declared(3))),
        ("cork", crate::fn_info!(ns_cork0, 0; with_declared(0))),
        ("uncork", crate::fn_info!(ns_uncork0, 0; with_declared(0))),
        ("destroy", crate::fn_info!(ns_destroy1, 1; with_declared(1))),
        (
            "setDefaultEncoding",
            crate::fn_info!(ns_chain1, 1; with_declared(1)),
        ),
    ]
}

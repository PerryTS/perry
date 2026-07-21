//! node:stream — Duplex prototype method table. Split out of
//! node_stream_readwrite.rs for the 2000-line file-size gate.
use super::*;

pub(super) fn duplex_methods() -> [(&'static str, StubFn); 44] {
    // Union of readable + writable, deduped (`on/once/off/addListener/
    // removeListener/removeAllListeners/emit/listenerCount/listeners/
    // destroy` appear once each).
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast2(ns_pipe2)),
        ("unpipe", cast1(ns_unpipe1)),
        ("wrap", cast1(ns_wrap1)),
        ("pause", cast0(ns_pause0)),
        ("resume", cast0(ns_resume0)),
        ("setEncoding", cast1(ns_set_encoding1)),
        ("isPaused", cast0(ns_is_paused0)),
        ("toArray", cast1(ns_iter_to_array)),
        ("map", cast2(ns_iter_map)),
        ("filter", cast2(ns_iter_filter)),
        ("reduce", cast3(ns_iter_reduce)),
        ("forEach", cast2(ns_iter_for_each)),
        ("find", cast2(ns_iter_find)),
        ("some", cast2(ns_iter_some)),
        ("every", cast2(ns_iter_every)),
        ("flatMap", cast2(ns_iter_flat_map)),
        ("take", cast1(ns_iter_take)),
        ("drop", cast1(ns_iter_drop)),
        ("iterator", cast1(async_iterator::ns_iterator1)),
        ("push", cast1(ns_push1)),
        ("unshift", cast1(ns_unshift1)),
        ("compose", cast1(ns_compose1)),
        ("write", cast3(ns_write3)),
        ("end", cast3(ns_end3)),
        ("cork", cast0(ns_cork0)),
        ("uncork", cast0(ns_uncork0)),
        ("destroy", cast1(ns_destroy1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
    ]
}

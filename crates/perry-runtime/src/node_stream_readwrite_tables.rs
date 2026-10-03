//! Readable/emitter/writable stream method tables, split from
//! node_stream_readwrite.rs for the 2000-line file-size gate. Still a child
//! module, so `use super::*` reaches the parent's private items.

use super::*;
use crate::closure::JsFunctionInfo;

/// The info of the stream method body `$body`, declaring `$n` JS parameters
/// (typed: any other signature does not compile).
macro_rules! ns_info {
    ($body:path, $n:tt) => {
        JsFunctionInfo::of($body as crate::fn_info!(@ty $n))
    };
}

/// The stream method bodies' infos: one per body, shared by every table below.
static NS_ON2_INFO: JsFunctionInfo = ns_info!(ns_on2, 2).with_declared(2);
static NS_ONCE2_INFO: JsFunctionInfo = ns_info!(ns_once2, 2).with_declared(2);
static NS_PREPEND_LISTENER2_INFO: JsFunctionInfo =
    ns_info!(ns_prepend_listener2, 2).with_declared(2);
static NS_PREPEND_ONCE_LISTENER2_INFO: JsFunctionInfo =
    ns_info!(ns_prepend_once_listener2, 2).with_declared(2);
static NS_OFF2_INFO: JsFunctionInfo = ns_info!(ns_off2, 2).with_declared(2);
static NS_REMOVE_LISTENER2_INFO: JsFunctionInfo = ns_info!(ns_remove_listener2, 2).with_declared(2);
static NS_REMOVE_ALL_LISTENERS1_INFO: JsFunctionInfo =
    ns_info!(ns_remove_all_listeners1, 1).with_declared(1);
static NS_EMIT_REST_INFO: JsFunctionInfo = ns_info!(ns_emit_rest, 2).with_rest(1);
static NS_SET_MAX_LISTENERS_INFO: JsFunctionInfo =
    ns_info!(ns_set_max_listeners, 1).with_declared(1);
static NS_GET_MAX_LISTENERS_INFO: JsFunctionInfo =
    ns_info!(ns_get_max_listeners, 0).with_declared(0);
static NS_EVENT_NAMES_INFO: JsFunctionInfo = ns_info!(ns_event_names, 0).with_declared(0);
static NS_LISTENER_COUNT_INFO: JsFunctionInfo = ns_info!(ns_listener_count, 1).with_declared(1);
static NS_LISTENERS_INFO: JsFunctionInfo = ns_info!(ns_listeners, 1).with_declared(1);
static NS_RAW_LISTENERS_INFO: JsFunctionInfo = ns_info!(ns_raw_listeners, 1).with_declared(1);
static NS_READ1_INFO: JsFunctionInfo = ns_info!(ns_read1, 1).with_declared(1);
static NS_PIPE2_INFO: JsFunctionInfo = ns_info!(ns_pipe2, 2).with_declared(2);
static NS_UNPIPE1_INFO: JsFunctionInfo = ns_info!(ns_unpipe1, 1).with_declared(1);
static NS_WRAP1_INFO: JsFunctionInfo = ns_info!(ns_wrap1, 1).with_declared(1);
static NS_PAUSE0_INFO: JsFunctionInfo = ns_info!(ns_pause0, 0).with_declared(0);
static NS_RESUME0_INFO: JsFunctionInfo = ns_info!(ns_resume0, 0).with_declared(0);
static NS_DESTROY1_INFO: JsFunctionInfo = ns_info!(ns_destroy1, 1).with_declared(1);
static NS_SET_ENCODING1_INFO: JsFunctionInfo = ns_info!(ns_set_encoding1, 1);
static NS_IS_PAUSED0_INFO: JsFunctionInfo = ns_info!(ns_is_paused0, 0).with_declared(0);
static NS_ITER_TO_ARRAY_INFO: JsFunctionInfo = ns_info!(ns_iter_to_array, 1).with_declared(1);
static NS_ITER_MAP_INFO: JsFunctionInfo = ns_info!(ns_iter_map, 2).with_declared(2);
static NS_ITER_FILTER_INFO: JsFunctionInfo = ns_info!(ns_iter_filter, 2).with_declared(2);
static NS_ITER_REDUCE_INFO: JsFunctionInfo = ns_info!(ns_iter_reduce, 3).with_declared(3);
static NS_ITER_FOR_EACH_INFO: JsFunctionInfo = ns_info!(ns_iter_for_each, 2).with_declared(2);
static NS_ITER_FIND_INFO: JsFunctionInfo = ns_info!(ns_iter_find, 2).with_declared(2);
static NS_ITER_SOME_INFO: JsFunctionInfo = ns_info!(ns_iter_some, 2).with_declared(2);
static NS_ITER_EVERY_INFO: JsFunctionInfo = ns_info!(ns_iter_every, 2).with_declared(2);
static NS_ITER_FLAT_MAP_INFO: JsFunctionInfo = ns_info!(ns_iter_flat_map, 2).with_declared(2);
static NS_ITER_TAKE_INFO: JsFunctionInfo = ns_info!(ns_iter_take, 1).with_declared(1);
static NS_ITER_DROP_INFO: JsFunctionInfo = ns_info!(ns_iter_drop, 1).with_declared(1);
static NS_ITERATOR1_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    async_iterator::ns_iterator1 as crate::codegen_abi::JsBody1<crate::closure::ClosureHeader>,
)
.with_declared(1);
static NS_PUSH1_INFO: JsFunctionInfo = ns_info!(ns_push1, 1).with_declared(1);
static NS_UNSHIFT1_INFO: JsFunctionInfo = ns_info!(ns_unshift1, 1).with_declared(1);
static NS_COMPOSE1_INFO: JsFunctionInfo = ns_info!(ns_compose1, 1).with_declared(1);
static NS_WRITE3_INFO: JsFunctionInfo = ns_info!(ns_write3, 3).with_declared(3);
static NS_END3_INFO: JsFunctionInfo = ns_info!(ns_end3, 3).with_declared(3);
static NS_CORK0_INFO: JsFunctionInfo = ns_info!(ns_cork0, 0).with_declared(0);
static NS_UNCORK0_INFO: JsFunctionInfo = ns_info!(ns_uncork0, 0).with_declared(0);
static NS_CHAIN1_INFO: JsFunctionInfo = ns_info!(ns_chain1, 1).with_declared(1);
static NS_CHAIN3_INFO: JsFunctionInfo = ns_info!(ns_chain3, 3).with_declared(3);

// Method table order determines packed-key order and shape-cache identity.

pub(crate) fn readable_methods() -> [(&'static str, StubFn); 39] {
    [
        ("on", &NS_ON2_INFO),
        ("once", &NS_ONCE2_INFO),
        ("prependListener", &NS_PREPEND_LISTENER2_INFO),
        ("prependOnceListener", &NS_PREPEND_ONCE_LISTENER2_INFO),
        ("off", &NS_OFF2_INFO),
        ("addListener", &NS_ON2_INFO),
        ("removeListener", &NS_REMOVE_LISTENER2_INFO),
        ("removeAllListeners", &NS_REMOVE_ALL_LISTENERS1_INFO),
        ("emit", &NS_EMIT_REST_INFO),
        ("setMaxListeners", &NS_SET_MAX_LISTENERS_INFO),
        ("getMaxListeners", &NS_GET_MAX_LISTENERS_INFO),
        ("eventNames", &NS_EVENT_NAMES_INFO),
        ("listenerCount", &NS_LISTENER_COUNT_INFO),
        ("listeners", &NS_LISTENERS_INFO),
        ("rawListeners", &NS_RAW_LISTENERS_INFO),
        ("read", &NS_READ1_INFO),
        ("pipe", &NS_PIPE2_INFO),
        ("unpipe", &NS_UNPIPE1_INFO),
        ("wrap", &NS_WRAP1_INFO),
        ("pause", &NS_PAUSE0_INFO),
        ("resume", &NS_RESUME0_INFO),
        ("destroy", &NS_DESTROY1_INFO),
        ("setEncoding", &NS_SET_ENCODING1_INFO),
        ("isPaused", &NS_IS_PAUSED0_INFO),
        // #1558: async iterator helpers; arities pad missing options args.
        ("toArray", &NS_ITER_TO_ARRAY_INFO),
        ("map", &NS_ITER_MAP_INFO),
        ("filter", &NS_ITER_FILTER_INFO),
        ("reduce", &NS_ITER_REDUCE_INFO),
        ("forEach", &NS_ITER_FOR_EACH_INFO),
        ("find", &NS_ITER_FIND_INFO),
        ("some", &NS_ITER_SOME_INFO),
        ("every", &NS_ITER_EVERY_INFO),
        ("flatMap", &NS_ITER_FLAT_MAP_INFO),
        ("take", &NS_ITER_TAKE_INFO),
        ("drop", &NS_ITER_DROP_INFO),
        ("iterator", &NS_ITERATOR1_INFO),
        // #1539 — push() backpressure return + readable.compose() instance form.
        ("push", &NS_PUSH1_INFO),
        ("unshift", &NS_UNSHIFT1_INFO),
        ("compose", &NS_COMPOSE1_INFO),
    ]
}

/// #5137: the bare `EventEmitter` surface — the same 15 listener/emit
/// methods that `readable_methods`/`writable_methods` share, minus all the
/// stream-specific entries. `install_event_emitter_prototype` puts ONE set of
/// them on `EventEmitter.prototype`, which a source-compiled `class X extends
/// EventEmitter` (e.g. commander's `Command`) inherits, without routing
/// through the handle-based `js_event_emitter_*` shim. The closures are the
/// generic `ns_*` emitter helpers, which keep all state in the receiver's
/// own `_events`, so they work unchanged on a plain object that never went
/// through a stream constructor.
pub(crate) fn emitter_methods() -> [(&'static str, StubFn); 15] {
    [
        ("on", &NS_ON2_INFO),
        ("once", &NS_ONCE2_INFO),
        ("prependListener", &NS_PREPEND_LISTENER2_INFO),
        ("prependOnceListener", &NS_PREPEND_ONCE_LISTENER2_INFO),
        ("off", &NS_OFF2_INFO),
        ("addListener", &NS_ON2_INFO),
        ("removeListener", &NS_REMOVE_LISTENER2_INFO),
        ("removeAllListeners", &NS_REMOVE_ALL_LISTENERS1_INFO),
        ("emit", &NS_EMIT_REST_INFO),
        ("setMaxListeners", &NS_SET_MAX_LISTENERS_INFO),
        ("getMaxListeners", &NS_GET_MAX_LISTENERS_INFO),
        ("eventNames", &NS_EVENT_NAMES_INFO),
        ("listenerCount", &NS_LISTENER_COUNT_INFO),
        ("listeners", &NS_LISTENERS_INFO),
        ("rawListeners", &NS_RAW_LISTENERS_INFO),
    ]
}

pub(crate) fn writable_methods() -> [(&'static str, StubFn); 22] {
    [
        ("on", &NS_ON2_INFO),
        ("once", &NS_ONCE2_INFO),
        ("prependListener", &NS_PREPEND_LISTENER2_INFO),
        ("prependOnceListener", &NS_PREPEND_ONCE_LISTENER2_INFO),
        ("off", &NS_OFF2_INFO),
        ("addListener", &NS_ON2_INFO),
        ("removeListener", &NS_REMOVE_LISTENER2_INFO),
        ("removeAllListeners", &NS_REMOVE_ALL_LISTENERS1_INFO),
        ("emit", &NS_EMIT_REST_INFO),
        ("setMaxListeners", &NS_SET_MAX_LISTENERS_INFO),
        ("getMaxListeners", &NS_GET_MAX_LISTENERS_INFO),
        ("eventNames", &NS_EVENT_NAMES_INFO),
        ("listenerCount", &NS_LISTENER_COUNT_INFO),
        ("listeners", &NS_LISTENERS_INFO),
        ("rawListeners", &NS_RAW_LISTENERS_INFO),
        ("write", &NS_WRITE3_INFO),
        ("end", &NS_END3_INFO),
        ("cork", &NS_CORK0_INFO),
        ("uncork", &NS_UNCORK0_INFO),
        ("destroy", &NS_DESTROY1_INFO),
        ("setDefaultEncoding", &NS_CHAIN1_INFO),
        ("_write", &NS_CHAIN3_INFO),
    ]
}

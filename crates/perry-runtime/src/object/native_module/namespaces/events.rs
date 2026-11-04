//! Item-level native namespace declarations for `events` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_EVENTS = "events" {
        births = [Object];
        members = {
            EventEmitter = (ref("node:events.EventEmitter"));
            EventEmitterAsyncResource = (ref("node:events.EventEmitterAsyncResource"));
            default = (ref("node:events.EventEmitter"));
            defaultMaxListeners = (10.0);
            usingDomains = (false);
            captureRejections = (false);
            captureRejectionSymbol = (Symbol.for("nodejs.rejection"));
            errorMonitor = (Symbol.for("events.errorMonitor"));
            init = (ref("node:events.init"));
            listenerCount = (ref("node:events.listenerCount"));
            on = (ref("node:events.on"));
            once = (ref("node:events.once"));
            addAbortListener = (ref("node:events.addAbortListener"));
            getEventListeners = (ref("node:events.getEventListeners"));
            getMaxListeners = (ref("node:events.getMaxListeners"));
            setMaxListeners = (ref("node:events.setMaxListeners"));
        }
    }
}

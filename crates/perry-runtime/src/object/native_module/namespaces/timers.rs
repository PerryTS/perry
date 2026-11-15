//! Item-level native namespace declarations for `timers` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_TIMERS = "timers" {
        births = [Object];
        members = {
            setTimeout = (ref("node:timers.setTimeout"));
            clearTimeout = (ref("node:timers.clearTimeout"));
            setImmediate = (ref("node:timers.setImmediate"));
            clearImmediate = (ref("node:timers.clearImmediate"));
            setInterval = (ref("node:timers.setInterval"));
            clearInterval = (ref("node:timers.clearInterval"));
            promises = (ref("node:timers.promises"));
        }
    }
}

crate::realm_decl! {
    namespace NM_TIMERS_PROMISES = "timers/promises" {
        members = {
            setTimeout = (ref("node:timers/promises.setTimeout"));
            setImmediate = (ref("node:timers/promises.setImmediate"));
            setInterval = (ref("node:timers/promises.setInterval"));
            scheduler = (ref("node:timers/promises.scheduler"));
        }
    }
}

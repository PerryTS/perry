//! Native instance layouts of node's emitter and stream classes (object
//! model §3.2). A layout names its class's sealed runtime prototype, so a
//! direct `new X()` is born on it with no prototype link to stamp. The key
//! sequence is node's enumeration order; slots are prefix-stable: Duplex and
//! Transform keep Readable's slots, and every stream keeps the emitter's
//! three. Members node creates only when an option or a listener asks for
//! them are optional: their slot holds the hole until filled, and the key is
//! absent meanwhile (`key_view`). A fill appends, as node's `[[Set]]` does
//! (`key_view::fill_hole`).
use super::*;

runtime_atom!(
    SI_ATOM_U_READABLE_STATE,
    "_readableState",
    "5f7265616461626c655374617465"
);
runtime_atom!(
    SI_ATOM_U_WRITABLE_STATE,
    "_writableState",
    "5f7772697461626c655374617465"
);
runtime_atom!(
    SI_ATOM_ALLOW_HALF_OPEN,
    "allowHalfOpen",
    "616c6c6f7748616c664f70656e"
);
runtime_atom!(SI_ATOM_U_CONSTRUCT, "_construct", "5f636f6e737472756374");

runtime_prototype_family!(
    WRITABLE_PROTO_FAMILY = INTRINSIC_SERIAL_WRITABLE_PROTOTYPE,
    TRANSFORM_PROTO_FAMILY = INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE,
);

const OPT: u8 = crate::object::key_attrs::ENTRY_OPTIONAL;

/// The slots every emitter and stream layout shares.
pub(crate) mod emitter_slot {
    pub(crate) const EVENTS: u32 = 0;
    pub(crate) const EVENTS_COUNT: u32 = 1;
    pub(crate) const MAX_LISTENERS: u32 = 2;
}

/// Readable's slots, which Duplex and Transform keep.
pub(crate) mod readable_slot {
    pub(crate) const STATE: u32 = 3;
    pub(crate) const READ: u32 = 4;
    pub(crate) const DESTROY: u32 = 5;
    pub(crate) const CONSTRUCT: u32 = 6;
}

/// Writable's own slots (a Writable is not a Duplex prefix).
pub(crate) mod writable_slot {
    pub(crate) const STATE: u32 = 3;
    pub(crate) const WRITE: u32 = 4;
    pub(crate) const WRITEV: u32 = 5;
    pub(crate) const DESTROY: u32 = 6;
    pub(crate) const FINAL: u32 = 7;
    pub(crate) const CONSTRUCT: u32 = 8;
}

/// Duplex's slots past Readable's; Transform keeps them.
pub(crate) mod duplex_slot {
    pub(crate) const WRITABLE_STATE: u32 = 7;
    pub(crate) const ALLOW_HALF_OPEN: u32 = 8;
    pub(crate) const WRITE: u32 = 9;
    pub(crate) const WRITEV: u32 = 10;
    pub(crate) const FINAL: u32 = 11;
    pub(crate) const TRANSFORM: u32 = 12;
    pub(crate) const FLUSH: u32 = 13;
}

// `new EventEmitter()`: node's `EventEmitter.init`.
runtime_layout!(
    EVENT_EMITTER,
    0x53490001,
    0,
    &raw const EVENT_EMITTER_PROTO_FAMILY,
    [NP_ATOM_U_EVENTS @ emitter_slot::EVENTS, NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT, NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS],
    realm = ["node:events.EventEmitter"]
);

// The legacy `new Stream()`: `EE.call(this, opts)` on `Stream.prototype`.
runtime_layout!(
    LEGACY_STREAM,
    0x53490002,
    0,
    &raw const STREAM_PROTO_FAMILY,
    [NP_ATOM_U_EVENTS @ emitter_slot::EVENTS, NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT, NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS],
    realm = ["node:stream.Stream"]
);

// `new Readable(opts)`: `_eventsCount` comes with the first listener.
runtime_layout!(READABLE, 0x53490003, 0, &raw const READABLE_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_READABLE_STATE @ readable_slot::STATE,
    NP_ATOM_U_READ @ readable_slot::READ => OPT,
    NP_ATOM_U_DESTROY @ readable_slot::DESTROY => OPT,
    SI_ATOM_U_CONSTRUCT @ readable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:stream.Readable"]);

// `new Writable(opts)`.
runtime_layout!(WRITABLE, 0x53490004, 0, &raw const WRITABLE_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_WRITABLE_STATE @ writable_slot::STATE,
    NP_ATOM_U_WRITE @ writable_slot::WRITE => OPT,
    NP_ATOM_U_WRITEV @ writable_slot::WRITEV => OPT,
    NP_ATOM_U_DESTROY @ writable_slot::DESTROY => OPT,
    NP_ATOM_U_FINAL @ writable_slot::FINAL => OPT,
    SI_ATOM_U_CONSTRUCT @ writable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:stream.Writable"]);

// `new Duplex(opts)`: Readable's slots, then the writable side's.
runtime_layout!(DUPLEX, 0x53490005, 0, &raw const DUPLEX_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_READABLE_STATE @ readable_slot::STATE,
    SI_ATOM_U_WRITABLE_STATE @ duplex_slot::WRITABLE_STATE,
    SI_ATOM_ALLOW_HALF_OPEN @ duplex_slot::ALLOW_HALF_OPEN,
    NP_ATOM_U_READ @ readable_slot::READ => OPT,
    NP_ATOM_U_WRITE @ duplex_slot::WRITE => OPT,
    NP_ATOM_U_WRITEV @ duplex_slot::WRITEV => OPT,
    NP_ATOM_U_DESTROY @ readable_slot::DESTROY => OPT,
    NP_ATOM_U_FINAL @ duplex_slot::FINAL => OPT,
    SI_ATOM_U_CONSTRUCT @ readable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:stream.Duplex"]);

// `new Transform(opts)`: Duplex's slots, then the transform hooks, which
// node sets before its `prefinish` listener creates `_eventsCount`.
runtime_layout!(TRANSFORM, 0x53490006, 0, &raw const TRANSFORM_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_READABLE_STATE @ readable_slot::STATE,
    SI_ATOM_U_WRITABLE_STATE @ duplex_slot::WRITABLE_STATE,
    SI_ATOM_ALLOW_HALF_OPEN @ duplex_slot::ALLOW_HALF_OPEN,
    NP_ATOM_U_READ @ readable_slot::READ => OPT,
    NP_ATOM_U_WRITE @ duplex_slot::WRITE => OPT,
    NP_ATOM_U_WRITEV @ duplex_slot::WRITEV => OPT,
    NP_ATOM_U_DESTROY @ readable_slot::DESTROY => OPT,
    NP_ATOM_U_FINAL @ duplex_slot::FINAL => OPT,
    SI_ATOM_U_CONSTRUCT @ readable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_U_TRANSFORM @ duplex_slot::TRANSFORM => OPT,
    NP_ATOM_U_FLUSH @ duplex_slot::FLUSH => OPT,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:stream.Transform"]);

// `new PassThrough(opts)`: Transform's slots on `PassThrough.prototype`.
runtime_layout!(PASSTHROUGH, 0x53490008, 0, &raw const PASSTHROUGH_PROTO_FAMILY, [
    NP_ATOM_U_EVENTS @ emitter_slot::EVENTS,
    SI_ATOM_U_READABLE_STATE @ readable_slot::STATE,
    SI_ATOM_U_WRITABLE_STATE @ duplex_slot::WRITABLE_STATE,
    SI_ATOM_ALLOW_HALF_OPEN @ duplex_slot::ALLOW_HALF_OPEN,
    NP_ATOM_U_READ @ readable_slot::READ => OPT,
    NP_ATOM_U_WRITE @ duplex_slot::WRITE => OPT,
    NP_ATOM_U_WRITEV @ duplex_slot::WRITEV => OPT,
    NP_ATOM_U_DESTROY @ readable_slot::DESTROY => OPT,
    NP_ATOM_U_FINAL @ duplex_slot::FINAL => OPT,
    SI_ATOM_U_CONSTRUCT @ readable_slot::CONSTRUCT => OPT,
    NP_ATOM_U_MAX_LISTENERS @ emitter_slot::MAX_LISTENERS,
    NP_ATOM_U_TRANSFORM @ duplex_slot::TRANSFORM => OPT,
    NP_ATOM_U_FLUSH @ duplex_slot::FLUSH => OPT,
    NP_ATOM_U_EVENTS_COUNT @ emitter_slot::EVENTS_COUNT => OPT,
], realm = ["node:stream.PassThrough"]);

#[cfg(test)]
#[path = "stream_instances_tests.rs"]
mod tests;

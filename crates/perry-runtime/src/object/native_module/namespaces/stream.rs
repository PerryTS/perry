//! Item-level native namespace declarations for `stream` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_STREAM_PROMISES = "stream/promises" {
        cell = "node:stream.Stream.promises";
        members = {
            finished = (ref("node:stream/promises.finished"));
            pipeline = (ref("node:stream/promises.pipeline"));
        }
    }
}

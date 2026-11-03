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

crate::realm_decl! {
    namespace NM_STREAM = "stream" {
        births = [Object];
        members = {
            Duplex = (ref("node:stream.Duplex"));
            PassThrough = (ref("node:stream.PassThrough"));
            Readable = (ref("node:stream.Readable"));
            Stream = (ref("node:stream.Stream"));
            Transform = (ref("node:stream.Transform"));
            Writable = (ref("node:stream.Writable"));
            _isArrayBufferView = (ref("node:stream._isArrayBufferView"));
            _isUint8Array = (ref("node:stream._isUint8Array"));
            _uint8ArrayToBuffer = (ref("node:stream._uint8ArrayToBuffer"));
            addAbortSignal = (ref("node:stream.addAbortSignal"));
            compose = (ref("node:stream.compose"));
            default = (ref("node:stream"));
            duplexPair = (ref("node:stream.duplexPair"));
            finished = (ref("node:stream.finished"));
            getDefaultHighWaterMark = (ref("node:stream.getDefaultHighWaterMark"));
            isDestroyed = (ref("node:stream.isDestroyed"));
            isDisturbed = (ref("node:stream.isDisturbed"));
            isErrored = (ref("node:stream.isErrored"));
            isReadable = (ref("node:stream.isReadable"));
            isWritable = (ref("node:stream.isWritable"));
            pipeline = (ref("node:stream.pipeline"));
            promises = (ref("node:stream.promises"));
            setDefaultHighWaterMark = (ref("node:stream.setDefaultHighWaterMark"));
        }
    }
}

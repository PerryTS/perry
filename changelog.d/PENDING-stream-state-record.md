### Changed

- Keep the runtime's private stream state in a native record reached from the stream itself (`meta.native_state` -> payload cell -> record) instead of ~60 `__perry…` hidden own properties per stream (decision 91 follow-up to #12104 G1/G2). A stream family's payload cell (zlib, the test rot13/count families) carries the record next to its codec; a plain JS Readable/Writable/Duplex/Transform gets a state-only cell with no payload and no hooks. A slot is a fixed index, so a state read or write is a few dependent loads plus the cell's exact malloc-parent barrier: no hidden-key interning through the SipHash map, no own-key scan of a wide instance, no shape transition, no define-property at construction.
- The record is traced and rewritten by the native-handle cell's GC descriptor and lives exactly as long as its stream. There is no side table, address map, latch or name check; the object and its cell are the authority.
- Node-visible state is unchanged: `_readableState`/`_writableState` views, `destroyed`, `readableEnded`, `allowHalfOpen`, … stay ordinary properties, and `Object.keys`, `JSON.stringify`, `for…in` and prototypes are as before. `Object.getOwnPropertyNames(stream)` no longer lists the 57–67 `__perry…` names it used to (toward node).
- The `JSON.stringify` stream probe, which runs for every serialized object, answers from the record instead of scanning every object's keys for the two flag names.

### Removed

- `STREAM_STATE_LAYOUT` and the construction-time definition of every hidden key; the `__perryStreamCaptureRejections` entry in `is_internal_runtime_key_bytes` (no object carries that key any more).

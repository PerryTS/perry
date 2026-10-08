### Changed

- Keep the runtime's private stream state in a native record reached from the stream itself instead of ~60 `__perry…` hidden own properties per stream (decision 91 follow-up to #12104 G1/G2). The record is a fixed, runtime-owned array of NaN-boxed words named by the stream's `meta.native_state`; a stream family (zlib, the test rot13/count families) keeps its payload cell in the record's `PayloadCell` slot, so `native_state -> record -> cell`. A slot is a fixed index: a state read or write is a few dependent loads plus the array's write barrier, with no hidden-key interning through the SipHash map, no own-key scan of a wide instance, no shape transition and no define-property at construction.
- The record is an ordinary arena object reachable only through its stream, so a short-lived stream's state dies in the minor that finds the stream dead (a malloc-cell home would have kept every young value it names alive until a full collection or the malloc-count sweep). Its edge is the meta record's existing traced `native_state` word. There is no side table, address map, latch or name check.
- One `native_state` word, one owner, with #12186's native-this alias: an object that is both a runtime stream and an aliased native construction (`Writable.call(this)` on an `http.ServerResponse.call(this, req)` object, a pipe/finished store on light-my-request's Response, stream state on an `http.Server.call(this)` object) keeps the alias in its record's `NativeAlias` slot. `ensure_record` moves an existing alias word into the record; `register_this_to_handle_alias` writes into the record when one exists; `object_alias` reads either. The alias stays a scalar (the GC never follows it). Before this, whichever came second was refused: an aliased object silently dropped its stream state.
- `native_payload::payload_cell_of_word` is the one resolution of a `native_state` word to its payload cell (a cell, a stream record's cell, or none for a weak collection's storage). `stream_hooks_of`, `payload_mut_attached`, `close_attached`, attach and the C-ABI cell lookups use it, which also stops them reading a WeakMap's storage as a cell. Attaching a payload to an initialized stream stores the cell in its record.
- Node-visible state is unchanged: `_readableState`/`_writableState`, `destroyed`, `readableEnded`, `allowHalfOpen`, … stay ordinary properties; `Object.keys`, `JSON.stringify`, `for…in` and prototypes are as before. `Object.getOwnPropertyNames(stream)` no longer lists the 57–67 `__perry…` names it used to (toward node).
- The `JSON.stringify` stream probe, which runs for every serialized object, answers from the record instead of scanning every object's keys for the two flag names.

### Removed

- `STREAM_STATE_LAYOUT` and the construction-time definition of every hidden key; the `__perryStreamCaptureRejections` entry in `is_internal_runtime_key_bytes` (no object carries that key any more).

Validation on perrymaster against origin/main aefd4acc2e (separate source trees and targets): stream gap subset (74 tests matching stream|pipe|pipeline|transform|readable|writable|duplex|zlib) 70 pass on both arms, 4 fail identically on both (http2 ×2, native_payload_zlib ×2), 0 output differences between arms. perry-runtime 5266/5266, perry-ext-zlib, perry-ffi pass; the two perry-stdlib thread-exit side-table failures are identical on main. All 15 stream sabotages turn red, including the new `record_trace` and `record_barrier` for `gc::tests::stream_state_record`.

n=5 interleaved instructions:u medians, default auto build, every output equal to node:

| program | main | head | Δ |
|---|---|---|---|
| buffer_heavy | 11,037,922,703 | 10,416,757,708 | −5.63% |
| fastify inject | 6,422,724,619 | 5,069,298,178 | −21.07% |
| tsc | 9,668,476,040 | 9,668,740,488 | +0.00% |
| Zod 5000 | 14,007,738,961 | 14,009,289,649 | +0.01% |
| qs parse | 21,927,889,285 | 21,928,265,589 | +0.00% |
| qs stringify | 51,560,533,575 | 51,556,309,886 | −0.01% |
| commander | 7,285,352,229 | 7,285,767,124 | +0.01% |
| hello | 257,240 | 257,205 | −0.01% |
| JS Transform micro (200k writes) | 50,301,820,365 | 30,533,032,007 | −39.30% |
| worker_heavy | 1,864,347,644 | 1,853,218,610 | −0.60% (spread ~2%) |

RSS is flat or lower everywhere (fastify −2.1 MB, Transform micro −18 MB, tsc +0.9 MB inside its trial range), with identical full-collection counts (buffer_heavy 36/36, worker_heavy 42/43, the rest 0/0).

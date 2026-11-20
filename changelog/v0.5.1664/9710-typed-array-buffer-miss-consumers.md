**Buffer-backed `Uint8Array` values now stay typed arrays when a kind-registry
lookup misses.** Perry deliberately represents JavaScript `Uint8Array` values
with `BufferHeader`, outside `lookup_typed_array_kind`; three consumers treated
that expected miss as a failed brand or receiver check.

The reflected `%TypedArray%.prototype` `length`, `byteLength`, `byteOffset`, and
`buffer` getters now accept both `Uint8Array` and Node `Buffer` receivers. They
also report real view offsets and the same stable backing `ArrayBuffer` as the
direct property path. Node-API's `napi_is_typedarray` and
`napi_get_typedarray_info` likewise report buffer-backed views and Buffers as
`napi_uint8_array`, including their backing identity, offset, and canonical data
span.

Finally, `Object.preventExtensions` and its `Reflect` counterparts record and
read the Uint8Array's side-table state, so it no longer remains extensible or
admits a fresh expando after successfully preventing extensions. `Reflect.set`
now also creates and updates its ordinary properties through the same Buffer
property table as direct assignment. The buffer sweeper removes the
address-keyed extensibility and TypedArray-property state on death.

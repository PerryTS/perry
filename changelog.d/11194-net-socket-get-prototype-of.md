Fix `Object.getPrototypeOf(socket)` answering `null` for a `net.Socket`
(connected, accepted, or unconnected `new net.Socket()`) although
`socket instanceof net.Socket` was `true`. A socket is a registry handle, and
the handle branch of `get_prototype_of_resolved`
(`perry-runtime/src/object/object_ops/prototype.rs`) only knew fetch values,
StringDecoder and X509Certificate. It now asks the net provider's socket
probe (the one `instanceof` already uses) and returns `net.Socket.prototype`,
read with an ordinary `[[Get]]` on the cached bound export so the prototype
exists even when nothing has read it yet.

undici 8.9.0's `util.destroy(socket, err)` reads
`Object.getPrototypeOf(stream).constructor` on every socket error path, so the
`null` replaced the real error with "Cannot read properties of null (reading
'constructor')". This is part of #11046. The error it was masking is the
missing `Buffer[Symbol.species]` (#11193). Covered by
`test-files/test_gap_net_socket_get_prototype_of.ts`.

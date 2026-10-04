Worker messages now use Node's structured clone. `postMessage` on a Worker,
`parentPort`, a MessagePort, a BroadcastChannel and `postMessageToThread`, and
`workerData`, clone every value Node clones: objects built by spread or
`Object.assign` keep all their fields, arrays keep their holes, shared
references and cycles stay shared, and `ArrayBuffer`, every typed array,
`DataView` (views keep their offset into the cloned buffer), `Buffer` (arrives
as a `Uint8Array`), `Map`, `Set`, `Error` (name, message, stack, cause),
`RegExp`, `Date` and `BigInt` arrive intact. ArrayBuffers in the transfer list
(or `transferList` for `workerData`) are detached on the sending side. A value
that cannot be cloned (a function, a symbol, a Promise) throws
`DataCloneError` instead of arriving as `undefined`. `perry/thread` captures
and results gain the same kinds.

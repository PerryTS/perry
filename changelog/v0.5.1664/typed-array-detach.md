A typed array whose `.buffer` was observed is no longer read through its
construction facts. Observing `.buffer` moves a fresh `new Float64Array(n)`
onto an external backing, and `buffer.transfer()` (or structuredClone or
postMessage with a transfer list) then detaches it. Compiled code kept reading
the old inline storage against the construction length. A specialized
`(a: Float64Array)` callee summed stale elements after a transfer (180 where
node prints `NaN`), and it missed writes made through the buffer or a
`subarray`. A local array read before a detaching call on a loop's back edge
returned the old elements instead of `undefined`. `Uint8Array` and
`Buffer.alloc` bindings had the same bug. Arrays that nothing can observe keep
their fast paths, and the benchmarks are unchanged.

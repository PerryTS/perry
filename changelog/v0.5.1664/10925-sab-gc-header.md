`SharedArrayBuffer` no longer lets one buffer's contents decide another
buffer's type. A SAB was handed to JavaScript as the address of a block with no
GC header in front of it, and several runtime paths read the eight bytes before
a value as its header. Those bytes are usually the tail of the previous SAB's
data, which a program can write through an ordinary `Uint8Array`. So writing a
byte into one SAB's own memory could make `Array.isArray` answer `true` for a
different SAB, and could make a `Map.prototype.get.call(sab, …)` brand check
follow fabricated pointers and crash. The answers also depended on how the
binary happened to be linked.

A SAB's backing now carries a real GC header, so every one of those reads gets
the honest kind and takes the ordinary buffer path. `Array.isArray(sab)` is
`false`, a collection method called on a SAB throws the `TypeError` node throws,
and none of it depends on neighbouring memory.

Sharing is unchanged: the backing is still one process-global, never-freed
allocation, two views over a SAB still alias the same bytes, a worker still sees
writes through a captured or module-level SAB, and `Atomics.wait` / `notify`
still rendezvous across agents on the same physical address.

The ordinary `[[Set]]` fast path takes only a POINTER_TAG'd target as a heap
object. It also admitted a target with clear top 16 bits ("a module-level
slot's raw pointer"), which is a Number (a denormal double) in value context,
and read a header at its bits: `EventEmitter.prototype.on.call(e, "x", f)`,
`addListener.call(e, ...)` and `stream.finished(e, cb)` crashed with SIGSEGV for
an address-shaped Number `e`. They now throw the TypeError they throw for every
other Number, as in Node.

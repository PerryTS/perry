Fixed a green blind spot in the GC store-site inventory: raw-pointer method
calls such as `slot.write(value)` and `src.copy_to(dst, count)` are now audited
alongside their `ptr::write` and `ptr::copy` free-function equivalents. The
checker distinguishes pointer-producing receivers from ordinary I/O, lock,
builder, and `MaybeUninit` writes, exercises every supported spelling in its
self-test, and records explicit safety rationales for the method-form stores
already present in the runtime.

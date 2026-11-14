A Buffer's `subarray` / `slice` result is a Buffer view again in the
whole-program facts (also when called with a spread argument), and `fill` /
`swap16` / `swap32` / `swap64` return the receiver. The facts modelled the view
as a container of primitives: a read through it (`v.readInt32BE(i)`) held
nothing, and `sum += v.readInt32BE(i)` was proven to stay the int32 0 (check
mode: rc 86 on `node-suite/buffer/test_buffer_b4_hoisted_view_gc.ts`; the
default output was node's).

#7302/#7305 replaced setjmp/longjmp exception lowering with `invoke`/`landingpad`
and deleted `volatile_setjmp.rs`. Two references to that machinery outlived it:

- `PERRY_SETJMP_VOLATILE` was still hashed into the object-cache key. The pass it
  gated no longer exists, so the field could never vary — dead state in a cache
  key, plus a test asserting its presence. Both removed. (Object-cache keys shift
  once as a result; the next build repopulates.)
- `native_emit.rs`'s module doc still claimed `has_try` functions render text
  "whose setjmp volatile pass needs whole-function analysis". That pass is gone
  and no such exception remains. Reworded to record the history rather than
  assert a present-tense behaviour that is no longer true.

The `_setjmp` link in `perry-ext-fastify` is deliberate and untouched — #7305
keeps the private Rust-side boundary trap, since Rust cannot catch a foreign
exception.

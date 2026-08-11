**Reading `.buffer` on a typed array now revokes its inline-storage proof.**
`js_typed_array_backing_buffer` materializes a backing `ArrayBuffer` for a typed
array that owned its bytes and rebinds the array to alias it, so element 0 stops
following the header. Codegen's proven-view tiers read
`header + 16 + idx*width` directly, on a proof taken at *construction* — a
literal length proves inline storage, and rightly so at that point — but nothing
revoked it when `.buffer` handed the storage out. So

```ts
const words = new Uint32Array(1);
const bytes = new Uint8Array(words.buffer);
words[0] = 0x01020304;          // wrote the ORPHANED pre-materialization bytes
bytes[0] + bytes[1] + bytes[2] + bytes[3];   // 0, not 10
```

and the reverse direction was equally invisible, while the buffer's *identity*
was already correct (`words.buffer === bytes.buffer`) — a lost alias rather than
a modelling gap. Writing *before* the second view was always right, because
materialization copies the current bytes, which made it look like a timing
quirk. The runtime guards its own inline reader with `PERRY_TA_VIEW_GUARD`;
these tiers are the compile-time proof that skips that check, so the hazard is
now recorded where the alias is created, via the existing
`downgrade_buffer_alias(..., MutableAlias)` path that reassignment, closure
capture and unknown-call escapes already use. A typed array whose `.buffer` is
never read keeps its fast path — the new
`test-files/test_gap_typedarray_buffer_aliasing_7219.ts` asserts that direction
for value, since a too-broad revocation would break it silently by staying
correct and getting slower. #7276 had closed only the ArrayBuffer-first shape
(#579), which never had an inline-storage proof to lose. (#7219)

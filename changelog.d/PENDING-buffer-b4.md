Buffer B4/B4c: one 16-byte cell for every byte type. Buffer, all TypedArray kinds, DataView, ArrayBuffer, SharedArrayBuffer and NativeArena views now share one layout: the GC header at p-8, element length at p+0, owner capacity (or a view's byteOffset) at p+4, and the single traced link at p+8. Owner bytes start at p+16, either inline or behind an out-of-line data pointer. The type byte carries the brand and the owner/view role. Header bits carry out-of-line/length-tracking, resizable, detached, frozen/sealed/non-extensible and nested pins. Byte cells are born old and never move, so pins and hoisted data pointers stay valid.

Named properties, `.buffer` identity, pin overflow and a custom prototype live in an ordinary null-prototype object bag, attached through the link word on first use. A view holds only (owner or bag, byteOffset, length) and reads detach, resize and out-of-bounds state from its owner at access time. `.buffer` is a length-tracking ArrayBuffer view over the original store. NativeArena is an out-of-line owner brand, and dispose detaches it.

Removed:
- VIEW_REGISTRY, BACKING_TO_VIEWS and RESIZABLE_BUFFER_MAX;
- BUFFER_AB_ALIAS and BUFFER_AB_ALIAS_EVER_SET;
- TYPED_ARRAY_VIEW_META, VIEW_META_COUNT and RESIZABLE_BUFFER_EVER_MARKED;
- the Buffer own-props map, TYPED_ARRAY_OWN_PROPS, PERRY_TA_OWN_PROPS_PRESENT and TYPED_ARRAY_NO_EXTEND;
- NativeArena's three registries and its view counter;
- the lazy TypedArray `.buffer` copy (TA_STORAGE_RESOLVED);
- the residual-prototype entries for byte cells;
- the emitted PERRY_U8_INLINE_CACHE and PERRY_TA_KIND_CACHE, together with js_u8_resolve_read_data.

The buffer-layout source ratchet drops from 272 sites to 15. The remaining 15 are in fetch and stream files and are left for their owners.

Emitted typed and byte reads guard one header word and then resolve the owner's data. Loops hoist (data, length) and keep both the receiver and the owner live across statepoints. They refresh after calls and polls.

`Buffer.allocUnsafe`, `Buffer.from` with a copy, and `Buffer.concat` place small buffers in node's allocPool:
- the pool is a per-agent ArrayBuffer sized by the live `Buffer.poolSize` (65536 by default on Node 26);
- allocations are 8-byte aligned and pool only below half the pool size;
- `Buffer.alloc`, Uint8Array and ArrayBuffer never pool.

Pool identity and byteOffset match node (#1225). As in node, one retained small view keeps its whole pool alive.

Also fixed: `Buffer.byteLength` of a non-u8 TypedArray now counts bytes.

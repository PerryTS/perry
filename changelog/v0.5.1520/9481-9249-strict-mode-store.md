**test(9249): opt the blocked-store case into strict mode (#9426 semantics)**

`reflect_define_property_non_writable_prototype_index_blocks_array_store`
asserted a `TypeError` from a sloppy-mode script. #9426 made a rejected
array-element write throw **only in strict mode** — which matches node:

| | output |
|---|---|
| Perry, with `"use strict"` | `TypeError 1 P` |
| Perry, as written (script) | `no error 1 P` |
| `node --experimental-strip-types`, same `.ts` | `no error 1 P` |

Perry and node agree exactly, so the code is right and the expectation was
stale. The test's purpose — a non-writable inherited index BLOCKS the store —
is still worth keeping, so it opts into strict mode rather than weakening the
assertion to the sloppy no-op.

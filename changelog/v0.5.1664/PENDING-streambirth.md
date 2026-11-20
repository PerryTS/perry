---
'@perry/runtime': patch
---

Replay stream constructor layouts through validated shape transitions, preserve Node's optional hook key order, and normalize state into the established stream record and public state shapes. Correct capture defaults and constructor option getter reads.

Hot state reads load the canonical public state slot after validating its recorded birth shape. Capture defaults live in EventEmitter.prototype's ordinary symbol property. Construction resolves its descriptor once per shape transition, validates the constructor/prototype stamps per use, and loads the live slot without invoking the default accessor. Prototype replacement, descriptor edits and direct symbol writes retain their ordinary semantics.

Recorded ordinary births derive their kind from the shape and revalidate it after allocation, preserving the merged EventEmitter birth replay fix.

Recorded object births validate the layout once before the existing nursery allocation leaf, with the shared slow allocator revalidating after collection. This removes redundant shape queries and allocator work while retaining the same allocation and slot-store authority.

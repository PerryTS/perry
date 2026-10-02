Declared class-field array calls such as `node.children.push(makeTree(...))`
now reach the existing guarded inline append path without requiring a typed
local in the source. The receiver is read once and the method is resolved before
the argument, preserving getters, receiver replacement, own/prototype method
overrides, subclasses, frozen arrays, and moving-GC roots and write barriers.

On #11743's unchanged cyclic workload (36,868,264 nodes), three rotated runs
per arm with matching `perry-dev` compiler/runtime builds reduced median CPU
from 10.65 s to 4.89 s; the typed-local diagnostic took 4.91 s. Peak RSS remained
essentially unchanged (130.63 MiB baseline, 130.41 MiB fixed). All full outputs
matched Node 26.5.1. Emitted-IR and semantic regressions cover the original
spelling, allocating recursive arguments, and lookup-first fallback behavior;
native and shadow moving-GC stress and static root checks pass.

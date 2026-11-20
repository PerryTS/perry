Declared class-field array calls such as `node.children.push(makeTree(...))`
now use guarded direct builtin dispatch without requiring a typed source local.
The receiver and method are resolved before a single shared argument-evaluation
block, preserving getters, receiver replacement, own/prototype overrides,
subclasses, frozen arrays, and moving-GC roots. The builtin calls the existing
`js_array_push_f64_spec` implementation, which handles forwarding, growth,
descriptors and write barriers once in the runtime instead of expanding the
append machinery at every unrolled or specialized call site.

On #11743's unchanged cyclic workload (36,868,264 nodes), three rotated runs
per arm with matching `perry-dev` compiler/runtime builds reduced median CPU
from 10.85 s to 5.57 s; peak RSS remained essentially unchanged (130.58 MiB
baseline, 130.42 MiB fixed). All full outputs matched Node 26.5.1. Application
object machine code grew by 396 bytes (1.1%), versus 4,068 bytes in the initial
expanded implementation. The linked executable adds 384 bytes of machine code
and 72 bytes of GC maps, with no change to its aligned file size. Many-site and
nested-unrolling probes reduce application-code growth from 34–40% to 7–8%.

Emitted-IR regressions require one argument emission and shared builtin calls,
including a 64-site nested-unrolling case. Semantic regressions, native and
shadow moving-GC stress, and static root checks pass. No version bump.

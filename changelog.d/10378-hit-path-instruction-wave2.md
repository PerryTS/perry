Second wave of the executed-instruction campaign (after #10295), aimed at the
three areas that audit deferred: parameter guards, per-element array work, and
key/string lookups. Across a 97-probe set called through a dynamic namespace
lookup, the summed per-call cost falls 20,308 -> 16,996 (-16.3%) with no probe
regressing beyond the noise floor.

A constant-key `in` now caches a presence answer on the receiver's ShapeId:
955 -> 30 instructions. Only positive answers about an OWN key are cached,
because that is the only claim no prototype mutation can falsify -- a negative
would be a statement about the whole chain, and there is no epoch to key one
on. Every way of losing the key either moves the ShapeId or raises
OBJ_FLAG_STABLE_TOMBSTONES, which the guard rejects.

Parameter guards stop walking descriptors nothing consumes. A clone consumes
one fact per parameter -- the declared type -- and never the descriptor's
field nodes, so an all-number class parameter is proved nominally from the
class id plus the typed-layout-intact bit, and the rule that let a loop in the
body license an unbounded per-element walk is gone. A 1,600-element `Pt[]`
parameter costs 1,146,528 instructions per call before and 21,553 after; a
`string[]` of the same length 213,233 -> 88,468; a class parameter with eight
number fields 3,194 -> 1,306. One non-`number` field on the chain puts the
whole chain back on the walk (control: 1,828 -> 1,844), because the intact bit
is a raw-f64 claim and says nothing about what a pointer slot holds.

Rest bundles are built the way array literals are -- one inline bump
allocation and N stores instead of `js_array_alloc` plus a per-element
`js_array_push_f64` that re-classified the receiver every time: `f(1, 2, 3)`
909 -> 85, `f(o, o, o)` 1,655 -> 486, with bundles wider than 16 left on the
old path. `map` resolves its result header once per element rather than three
times, keeping the full protocol (canonicalize, retire the numeric claim,
layout note, remembered-set edge): `a.map(v => v + 1)` over 16 elements
6,068 -> 4,028. The packed loop stops re-deriving its element base per element
and its counter read shades no GC root, restricted to offset 0 because
`arr[i +/- c]` can leave the array and reach the prototype chain.

Smaller runtime paths: an ASCII string index answers from the short-string
value instead of a four-call chain ending in a thread-local table (172 -> 123);
`[[HasProperty]]` resolves the recorded prototype only where it is read;
the concat chain formats number parts in place instead of building an
intermediate heap string; `instanceof`'s `util.inherits` escape hatch becomes
a process-wide latch instead of two registry probes per miss (miss 1,199 ->
1,081, hit unchanged); and a subclass `pop` flushes store plans only when it
actually retires a proof.

One change carries no measured win and its commit message says so: skipping
the re-registration of an unchanged class parent edge removes a process-global
prop_plan epoch bump and a CLASS_REGISTRY write lock from the outlined
allocation entry, but every allocation loop that could be built takes the
inline allocator instead, which never calls register_class.

Three spec divergences found while measuring are filed, not fixed: #10364
(`instanceof` against a Proxy right-hand side segfaults), #10365 (four
divergences from the spec's prototype walk), #10366 (`in` does not reach
`Function.prototype`). All reproduce on unmodified main.

Fix #12029: each evaluation of a class owns its prototype's properties.

The prototype objects and their slots were already distinct. The leak came
from property misses falling back to the class-id declaration prototype,
which is also the shared first evaluation's prototype. A property added to
that first prototype consequently appeared on later evaluations. Callable
`defineProperty` replacements could also update the shared registry, and
dynamic `super` calls could resurrect a deleted method from the template.

Treat an evaluation's recorded prototype chain as authoritative, including
misses and deleted accessors. Keep callable replacements and deletion
bookkeeping local to evaluation prototypes, and resolve dynamic `super`
through the evaluation's actual home prototype. Deleting a member of the
shared first evaluation preserves the ClassBody order used to build later
evaluations. Retain the template birth
shapes, direct constructor routes, and guarded method fast paths.

Add a Node parity regression covering both the reported arrow mixin and the
shared-first class-declaration case, property additions and deletions,
accessors, enumerable keys, independent statics, three-level mixin chains,
and `instanceof`. The minimal arrow example already matches Node on the
pinned main revision; the shared-first case and deleted parent method make
the expanded regression fail there. A runtime unit test checks that a fresh
prototype miss cannot read properties added to the declaration prototype.

Validation: the expanded regression matches Node 26.5.1 and fails on pinned
main. Deliberately re-enabling the shared declaration-prototype fallback
reproduces the leak; restoring the fix restores parity and identical release
binaries. The full runtime/HIR/codegen run has 8,452 passes and zero failures,
and all 158 class/fresh/mixin gap tests match Node with zero regressions.
Three-run instruction measurements preserve the fresh-class fast paths:
freshnew/freshcall/evalnew/ctor/stat and the #11967 call-cost repro differ
from main by -0.39% to +0.10%, within measurement noise. tsc, Zod ×5000,
qs parse/stringify, commander, and hello all match Node. Three-run median
instruction changes are within ±0.02% for the larger programs; hello is
+0.19%, within the observed startup variation. Formatting checks pass.

Array performance: an erased Array declaration admits object-backed Array
subclasses and typed arrays, so a canonical integer key on such a receiver now
brands the receiver once and takes the receiver-unknown numeric read tiers
(inline typed-array read, dense-subclass shape cache, complete dispatcher)
instead of the plain-array tier's out-of-line feedback fallback; and the
guarded in-bounds element store decides inline — with the exact
pointer-bearing classification of the old and new values plus the array's
element-shape bit — whether the GC layout note has any work before calling it.
wolf-ecs (noctjs/ecs-benchmark) on the Mac mini reference box: add/remove
-3.2% then -2.3%, entity-cycle -3.7% then -2.0%, each 11/11 paired wins,
semantics probes byte-identical to Node.

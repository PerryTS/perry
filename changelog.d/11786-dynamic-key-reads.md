Computed string-key reads (`o[k]` with a runtime string `k`) are answered from
the receiver's shape (#10753). The key is compared as one word per position
against the shape's canonical key list (which holds each key's atom, the value a
pooled key literal is), and an absent key is answered from a confirmed verdict
filed in the megamorphic read stub under the receiver's ShapeId and validated on
every use against `%Object.prototype%`'s ShapeId, the facts of the A2 depth-1
absent holder entry. A function-local `const K = "lit"` used as a key now folds
to the static read `o.lit`, as a module-level one already did (#10761).

Per read on `{a..h}` (instructions:u, main -> this change; node in brackets):
`O[W[i]]` all present 777 -> 182 (165); half absent 4,676 -> 260 (197);
`O[K]` with a local const K 803 -> 9 (12).

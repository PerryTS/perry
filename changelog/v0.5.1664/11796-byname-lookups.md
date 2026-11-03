Static-key reads that miss a read site's own cache are answered from holder
shapes in more cases (#10495, #10497). A site's holder entry now reads a key
that lives in the holder's spill storage (`%Object.prototype%`'s
`constructor`, keys added to a class prototype after it was built), holds one
inherited data answer for up to ten receiver shapes, and covers plain
functions: a function on its base shape pins its own keys and its
`[[Prototype]]`, so `fn.k` is answered from `%Function.prototype%`'s and
`%Object.prototype%`'s shapes. Declared-class instances get the same answers
from the GC-leaf read front, their direct prototype link proved by the class
lookup-surface generation instead of a registry probe per read, and a class
whose prototype object was never materialized gets it on the first miss.
Data attributes (non-enumerable, read-only) no longer refuse an inherited read,
only accessors do. `Object.getPrototypeOf` of a plain object answers from its
shape instead of reading `obj.constructor.prototype` by name.

Per iteration (instructions:u, #11786 base -> this change; node in brackets):
#10495 proto_data3 18,787 -> 556 (24), absent3 3,009 -> 525 (13);
#10497 fn_missing_prop 20,156 -> 167 (30), ctor_isBuffer 26,290 -> 271 (38),
axios_isBuffer 35,560 -> 542 (84), ctor_eq_Object_mono 5,891 -> 1,327 (31),
getProto_eq 7,489 -> 3,443 (220).

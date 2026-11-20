**Method calls on instances of a class evaluated more than once are answered by the instance's shape: a mixin class's `this.m()` is ~11× cheaper, and prettier formats ~17% faster** (Refs #11769, #11932).

An instance of a per-evaluation class (a mixin `B => class extends B { … }`, such as babel's parser classes inside prettier) inherits from its evaluation's prototype. A method call on it could not use the call site's learned receiver word or its chain memo: both refused a receiver with a recorded prototype or a private brand, so every call ran the whole dispatch tower and walked the prototype chain by name.

The instance's ShapeId already names its evaluation's prototype (the prototype's serial is part of the shape's prototype identity), so each evaluation's instances have their own ShapeIds. The receiver guard now admits a recorded prototype when the shape states it, and no longer consults the brand. The by-shapes chain walk starts at that prototype. The call site's chain memo keys a way on the evaluation's receiver word and calls the function object that the prototype's slot holds. The class-direct learned word (which calls the class's compiled body) is still learned only for receivers of the class's own prototype.

| row | before | after |
|---|---|---|
| fresh class `this.step(i)`, instructions per call | 8,804 | 808 |
| same code, class declared once | 151 | 151 |
| prettier (8 files × 2): instructions / wall | 29.90 G / 2.52 s | 23.68 G / 2.10 s |

tsc, Zod, qs, commander and hello: instructions within ±0.11%, full collections unchanged, RSS within noise.

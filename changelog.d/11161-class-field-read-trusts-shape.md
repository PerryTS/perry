Class-field reads whose class the compiler knows (`this.a` in a method) no
longer pay more than the generic inline cache. A dedicated read guard checks
the receiver with one biased unsigned compare, then compares only the ShapeId
for boxed fields: the shape already implies the GC kind, forwarding,
descriptor and tombstone state. `number` fields keep the class-id and
typed-layout checks. The hit path drops from 23 instructions to 11 (boxed) and
16 (number). Classes with subclasses the guard cannot cover now use the
generic inline cache instead of missing on every read.

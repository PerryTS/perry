### Performance — Array-subclass numeric indexing

Numeric reads from a stable `class X extends Array` instance now use an exact
class-and-ShapeId inline cache and load dense own elements directly from their
object slots. The guarded path retains generic semantics for holes, accessors,
prototype changes, proxies, forwarding, and real-Array element-kind changes,
while removing per-index string creation and generic property lookup from hot
ECS loops.

The Wolf-shaped #8655 reproducer (1,000 entities and 2,000 system iterations)
improves from a 1,064.0 ms median to 149.1 ms on Windows, a 7.1x speedup, with
identical output.

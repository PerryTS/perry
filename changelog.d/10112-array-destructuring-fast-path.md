### Performance

Array destructuring no longer drives the spec iterator protocol when the source
is a spread-free array literal written in place or a value whose static type
proves a plain `Array`. Both `[x, y] = [y, x]` and `const [a, b] = pair(i)`
previously paid a `GetIterator` call, one `iteratorNextResult` per element —
each allocating a `{ value, done }` result object — two property reads off that
result, and an `IteratorClose`, none of which is observable for an array whose
`Array.prototype[Symbol.iterator]` has not been replaced.

The lowering now emits both arms and branches on the same runtime guard that
`for…of` over a proven array uses (`Expr::ArrayIterationPatched`, a volatile
read of the runtime's sticky `PERRY_ARRAY_PROTO_ITERATOR_PATCHED` byte); no
runtime change was needed. For a literal source the fast arm spills the
literal's elements into temps and never builds the array at all, so the swap
loop's one GC allocation per iteration disappears — a 10,000,000-iteration
`[a, b] = [b, a]` loop runs 7,952 collections before the change and 6 after,
the same 6 it runs at 10,000 iterations. For a proven array it reads elements
by index, re-reading `length` per element exactly as `IteratorStep` does.

The branch is per element rather than around the whole pattern. A pattern
DECLARES bindings, so lowering it twice would give each binding two `LocalId`s;
per-element branching also preserves the spec's interleaving, which an eager
"pull N values, then bind" arm would lose — `let [a = f(), b] = src` still
evaluates `f()` between producing element 0 and element 1.

Measured against Node 26.5.1 on an Apple M1 Max (host under heavy concurrent
build load, so the absolute times are inflated; checksums matched at every
size):

| workload | n | before | after | speedup | before ÷ node | after ÷ node |
|---|---:|---:|---:|---:|---:|---:|
| `iteration-destructuring-swap` | 1,000 | 2.393 ms | 0.057 ms | 42.2x | 81.8x | 1.94x |
| `iteration-destructuring-swap` | 100,000 | 334.6 ms | 7.38 ms | 45.3x | 104.1x | 2.30x |
| `iteration-destructuring-swap` | 1,000,000 | 3071.9 ms | 105.0 ms | 29.3x | 102.6x | 3.51x |
| `iteration-destructure-return` | 1,000 | 5.199 ms | 0.076 ms | 68.2x | 101.0x | 1.48x |
| `iteration-destructure-return` | 100,000 | 457.0 ms | 7.70 ms | 59.3x | 61.5x | 1.04x |
| `iteration-destructure-return` | 1,000,000 | 4673.5 ms | 191.3 ms | 24.4x | 90.7x | 3.71x |

A rest element, a nested pattern, an empty pattern, a generator, a `Set` / `Map`
/ string and any source without a static array proof keep the unguarded iterator
lowering, statement-for-statement identical to before.

### Fixed

A `for…of` over a statically-proven array ignored a replaced
`%ArrayIteratorPrototype%.next`, iterating the unpatched elements where Node
iterates the patched ones — the half of the iteration protocol #7760's guard did
not cover, because the runtime detects a replaced `next` per `.next()` call and
an index loop never calls it.

The exported guard byte (renamed `PERRY_ARRAY_ITERATION_NOT_PRISTINE`, since it
no longer means only "`Array.prototype[Symbol.iterator]` was replaced") is now
also set when the array-iterator prototype object escapes to user code through
`Object.getPrototypeOf` / `Reflect.getPrototypeOf` — the only way to name that
object in order to patch it. Setting it on escape rather than on the write is
deliberate: the object is an ordinary object, so a precise hook would have to
cover every mutation funnel and missing one fails silently toward a wrong
answer, whereas over-approximating costs the index arm only in programs that
introspect an array iterator. The Rust-side `ARRAY_PROTO_ITERATOR_MODIFIED` bool
keeps its original narrow meaning, so the spread and `js_get_iterator`
delegation paths are unchanged.

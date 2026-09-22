**Removed the native `lru-cache` binding** — `import { LRUCache } from "lru-cache"` now resolves to
the real npm package, compiled from source. Native `instanceof` threw `Right-hand side of
'instanceof' is not callable` and `constructor.name` was undefined (the handle isn't a real class
object); `cache.forEach(...)` silently visited nothing and a `dispose` callback was never invoked,
where npm's real implementation does both. Also removes the dedicated #10293 native-subclass
machinery (`class X extends LRUCache` support for a binding with no runtime class value) — the real
compiled package's `LRUCache` is an ordinary JS class, so subclassing needs no special support at
all. Fixes #10685. Requires #10439's import-provenance fix (#10699) to reach the real package at its
default import name.

Acceptance against the real npm `lru-cache@11.5.2` (installed as the only dependency, **no**
`perry.compilePackages` entry) vs `node --experimental-strip-types` on the pinned Node 26.5.1:
**byte-identical output**, covering `set`/`get`/`has`/`delete`, recency eviction at `max`, `peek`,
object-value identity and mutation through the cached reference, `keys`/`values`/`entries`/
`forEach`, a `dispose` callback firing on eviction, npm's constructor `TypeError`s (`{}` and the
legacy positional form), `instanceof`, `constructor.name`, and `class Sub extends LRUCache`.

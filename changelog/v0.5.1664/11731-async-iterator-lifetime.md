Fix the async iterator lifetime leaks behind MongoDB's growing command listeners and retained response buffers (#11726).

- Lower `for await` with a lifetime-scoped `try`/`finally` in both module and function bodies. Closing a suspended generator now closes its inner iterator, including optional `return` methods, receiver preservation, and throw-completion precedence. Normal exhaustion and failed `next` calls do not close the iterator.
- Continue unwinding enclosing generator finalizers after an awaited finalizer finishes, instead of returning immediately and skipping outer cleanup.
- Keep the original throw continuation on the generator instance and queued requests, rather than permanently rooting every generator from idle queue metadata. Pending requests remain GC roots.
- Add Node parity regressions for nested iterator cleanup, awaited finalizer chaining, and collection of abandoned generators, plus HIR and runtime root-scanner tests.

Linux MongoDB insert/find validation: instructions fall from 207.7M to 37.4M per operation; peak RSS falls from 1,005 MiB to 205 MiB (1.78× Node); all GC buckets total 4.04% of instructions, below 10%. Full measurement details are in `benchmarks/packages/profile/issue-11726/`.

No version bump.

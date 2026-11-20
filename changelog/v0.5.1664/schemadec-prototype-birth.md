Reduce repeated work when schema decoders allocate wrapper objects with
`Object.create`. A prototype whose existing owner record already carries a
completed mark returns that record without reopening runtime handle scopes.
The first mark still installs its semantic shape and lifetime policy through
the existing shared path. No cache, registry or per-site mechanism is added.

Carry the resolved birth's prototype identity with its scalar ShapeId. A
floor birth checks that identity and the shape's continued presence instead
of resolving and comparing its complete immutable tuple a second time.
Foreign prototypes and retired shapes still use the sole shape interner.

The schema-decode profiling and acceptance harnesses retain symbols without
enabling debug call-location instrumentation, subtract two decode counts,
and compare isolated builds against Node with THP disabled.

Regenerate the Linux GC call-effects table from the final x86-64-v3 release
archives, including two pre-existing unsafe classifications. macOS and
Windows tables are not regenerated.

On qb6, the two changes remove 66 and 24 instructions respectively per
prototype birth. Decode-only Callgrind subtraction saves 4.20 million
instructions per 2,000-item decode (1.43%); the interleaved five-run,
THP-disabled full drivers save 1.18% on schema-record and 0.62% on Effect.
The four class micros remain flat. Runtime and codegen library failures
match the baseline; disabling the birth identity guard in a test-binary
copy makes the new foreign-prototype control fail as intended.

`perry-runtime` builds for 32-bit-pointer (ILP32) targets again — wasm32 and
watchOS `arm64_32` — as phase 1 of the standalone WASI target (#11375,
#11376). Ten const-evaluated 64-bit assumptions now state the layout each
target actually has (every LP64 value is unchanged): the box-capture record's
release flag and tag bits come from `usize::BITS`, box cells are asserted as
8 bytes holding a `usize` link, `ObjectHeader.meta` sits at
`16 - size_of::<usize>()`, the `MapHeader`/`SetHeader` layouts are pinned per
pointer width (LP64 block kept verbatim for codegen's source-reading test),
the `HotTls` and `ShadowStackState` codegen-contract offsets scale with pointer
width, and `alloc_census` hashes addresses in `u64`.

Because generated code is still LP64-only — the shadow-stack frame push and
inline slot stores, `MapHeader::used` and `HotTls` reads bake in 64-bit
offsets, and runtime pointers cross the ABI as `i64` — `compile_module` now
refuses ILP32 triples with an error pointing at #11378 instead of
miscompiling silently. `arm64_32` could not build its runtime before this
change and the default watchOS device target is `aarch64`, so nothing that
worked is affected.

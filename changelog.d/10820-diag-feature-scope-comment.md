**docs(runtime): correct the `diagnostics` feature comment's scope.**
`crates/perry-runtime/Cargo.toml`'s comment above `diagnostics = []` asserted
"None are on a hot path". That is true of the things it enumerates
(`PERRY_GC_DIAG`, the typed-feedback dump, the v8 heap-snapshot builder,
`process.report`) and false as a blanket statement, because the feature does
**not** gate `hot_diag` at all: `lib.rs` declares `pub mod hot_diag;` with no
cfg, the file contains zero `cfg(feature` occurrences, and `enum_on()` is
called per string concat (`string/concat.rs:688`, `:1052`) and per property
enumeration in **every** build, shipped included.

This matters because the comment is load-bearing in people's reasoning: a peer
session quoted a neighbouring `hot_diag` comment as evidence about generated
code and proposed a fix on that basis. Comments here are read as evidence, so
a blanket claim that is only true of an enumerated subset is worth narrowing.

**No performance claim attached, deliberately.** The obvious follow-up —
collapsing `enum_on()`'s `OnceLock`-probe-plus-`AtomicBool`-load into a single
three-state `AtomicU8` with a `#[cold] #[inline(never)]` resolve arm — was
implemented and measured against its exact parent commit: **399.76 → 401.19
instructions per short concat**, bare-loop control 3.01 / 2.99 in both arms.
That is +0.4% on a ~400-instruction operation, indistinguishable from zero, so
the change was reverted rather than shipped as churn.

The instructive part is why the theory was wrong. A peer measured a real win
(zod −5.2%, an S40 fixture −12.4%) from *deleting* an early-returning
diagnostic gate. That win came from removing the call and its inlining
barrier entirely — not from making the gate's body cheaper. Saving one load
out of four hundred instructions is below what any probe here resolves.

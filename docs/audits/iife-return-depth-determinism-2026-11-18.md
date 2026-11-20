# IIFE return-kind memoization: depth-sensitive proofs

Measured compiler commits: baseline `8e2ac851ef305606447d9d8a712840aa17febc96`
and candidate `19bc1bbba11673767ee6bd49411ab84d07eb34f1`.

## Defect and fix

`collectors/typed_locals.rs::Env::kind` caches a directly invoked closure's
return kind. The computation depends on both the closure's function ID and
the expression traversal depth: `iife_return_kind` refuses a proof beyond
depth four. Caching by function ID alone lets the first lookup supply the
answer for a clone at a different depth. Candidate locals are visited through
a randomly seeded hash set, so that first lookup can change between runs.

The cache key is now `(func_id, depth)`. The analysis limits and conservative
fallback are unchanged. This changes compile-time inference; it adds no runtime
check, dependency, API, environment knob, or version change.

## Unit evidence

The regression places one directly invoked closure, returning an array, in
two initializers. Both copies retain function ID 60. The first is shallow;
five sequence expressions wrap the second. Only local 1 has a proof within
its depth budget. Each successful test checks the exact array set `{1}` in
64 fresh analyses.

Against the original production collector, all ten fresh test processes fail
(exit 101). Six infer `{}` and four infer `{1, 2}`. The other 17 typed-local
tests pass. Against the candidate, all ten fresh processes pass, as does the
complete 18-test `collectors::typed_locals::tests` group.

The test executable is built with
`cargo test --release -p perry-codegen --lib --no-run --message-format=json`;
the selected tests are then invoked directly. The baseline negative control
adds the new test to the baseline checkout without changing production code.

## Scope of the investigation

This proves an order-dependent inference defect. It does not establish the
cause of the six application functions that motivated the investigation.

The program-wide array-descriptor predicate is a membership test on a shared
`RwLock` vector, published before code generation. Its producer sorts
prototype names and appends the descriptor marker by set membership. That
consumer does not depend on hash iteration or callable-worker snapshots.

A public pg-topo/effect probe emits the three relevant public closures. Its
topo-sort, fish, and config closures contain respectively 33, 15, and 2
element-kind queries before LLVM optimization, counting both `call` and
`invoke`; machine code contains 28, 10, and 1. LLVM therefore eliminates some
queries, but equal public inputs and outputs do not locate the original
application's first divergent decision.

A separate temporary native-home diagnostic compares retained IR and assembly
in ten fresh processes per arm, with 32 iterations per process. An observable
use keeps sixteen homes live; decoded GC records assert that the subject
survives optimization. Both arms produce identical retained IR and assembly.
The initial fixture incorrectly left those values unused, and its live-root
assertion rejected the resulting empty GC records; those attempts are not
counted as determinism evidence.

Replacing only the root-reload integer-register identity barrier with a
`ptrtoint` in the diagnostic removes two `vmovq` instructions and ten bytes:
923 bytes with the barrier, 913 without it. This demonstrates the transfer
mechanism, not why an application compile retains or avoids it. The production
barrier prevents stale relocation reuse and remains unchanged. No speculative
native-home ordering change is included.

## Public reproduction and acceptance

Ten compiles per arm with default module workers and ten with one worker all
match Node 26.12.1. Every run has one binary hash and one 149-file LLVM
aggregate hash, identical across both arms. The exact first-two module diffs
for topo-sort, fish and config are empty in all four groups. No end-to-end
variability is reproduced.

| Arm | Module jobs | Compiles | Binary hashes | LLVM hashes | Node matches |
| --- | --- | ---: | ---: | ---: | ---: |
| main | default | 10 | 1 | 1 | 10/10 |
| main | 1 | 10 | 1 | 1 | 10/10 |
| fix | default | 10 | 1 | 1 | 10/10 |
| fix | 1 | 10 | 1 | 1 | 10/10 |

Binary SHA-256: `881132cb05e2ec4e4b89cbb0a117c248a498471e5d63af897b939c802484f7ac`.
LLVM aggregate SHA-256: `90845da2bf166c1e917be9527f867ce58e0e7137373c3b9b679b67912f09defc`.

Both arms pass seven focused IIFE/array/exception parity cases. The supplied
effect fixture matches Node byte for byte. A single interleaved stripped-binary
measurement session uses CPU 30, disabled ASLR, three instruction samples
(extended to five when the candidate median is higher), and ten warm peak-RSS
samples (thirty for perft and evaluate 1). Every measured output matches Node,
including the required search checksum and principal variation.

| Workload | Instructions median main → fix | Delta | RSS median main → fix (KiB) | RSS max main → fix (KiB) |
| --- | ---: | ---: | ---: | ---: |
| search | 14,247,546,164 → 14,247,547,517 | +0.000009% | 22312 → 22312 (+0) | 22312 → 22312 (+0) |
| perft | 294,326,841 → 294,326,873 | +0.000011% | 14592 → 14592 (+0) | 14592 → 14592 (+0) |
| evaluate | 1,634,383,917 → 1,634,383,910 | -0.000000% | 14848 → 14848 (+0) | 14848 → 14848 (+0) |
| evaluate1 | 28,274,608 → 28,274,526 | -0.000290% | 7936 → 7936 (+0) | 7936 → 7936 (+0) |
| legal | 399,743,370 → 399,743,215 | -0.000039% | 14660 → 14656 (-4) | 14660 → 14660 (+0) |
| hello2 | 999,092 → 999,194 | +0.010209% | 4864 → 4864 (+0) | 4864 → 4864 (+0) |

| Binary | Bytes main → fix | Delta | Identical SHA-256? |
| --- | ---: | ---: | --- |
| chess | 8,244,000 → 8,244,000 | +0 | yes |
| hello2 | 6,378,888 → 6,378,888 | +0 | yes |

The performance gate passes: byte-identical binaries, no RSS increase, and
instruction differences within noise (maximum median increase 102 instructions,
0.010210%, for hello2). All raw samples are retained in the lane handoff.

Independent release builds use the same compiler, static runtime/stdlib wrapper
and GC-effects package set, with fresh sidecars and explicit runtime directories.
Every Perry compile disables its cache. Mac `cargo check -p perry-codegen --lib
--tests`, formatting and whitespace checks pass. Full suites and full lint
gates are outside this investigation's lean test scope.

Outcome: **NO-REPRO-FIX**. The IIFE inference defect is independently proven;
the original six function differences remain unexplained.

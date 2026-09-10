# Unchanged-main rebuild control (R19)

**The independently rebuilt compiler and both static archives are byte-for-byte identical to the original reference.** Build drift does not explain the recurring control regressions in this environment. R18 remains rejected; this result does not establish why its unchanged object-parse path slowed down.

Both builds use clean main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531), the same worktree and the exact production command:

```sh
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
```

The original build started at 15:48:26 UTC on 2026-09-10 and took 338.89 seconds. The independent rebuild started at 22:57:23 UTC and took 333.56 seconds. Before rebuilding, only the mtimes of the compiler and two static-wrapper entrypoints were refreshed; their source hashes stayed unchanged. All three output mtimes are later than the corresponding build start. No profile, codegen-unit, feature or GC flag override was introduced. Both original archives carry the expected main Git build stamp.

| Artifact | SHA-256, identical in both builds |
|---|---|
| Compiler | `ade63eddfdeca03926039d10e3310f9255d52c5b2721ccd2b0882f042a689156` |
| Runtime archive | `b2be9b8596982f5a1d475d666ada79ab2a78b9c8500b2c0d71d941a9f8d96458` |
| Stdlib archive | `61fac03f0405a48b8d390958e3002246061db6e009fcd078fe61a98d1a1068b0` |

[Build provenance](results/rebuild-control-r19-validation/build-provenance.json), [original build](results/rebuild-control-r19-validation/main-build-provenance.json), [independent hash comparison](results/rebuild-control-r19-validation/rebuild-comparison.json), [source and freshness preparation](results/rebuild-control-r19-validation/base.json), [original archive stamps](results/rebuild-control-r19-validation/reference-build-stamps.json).

## What this does and does not settle

There is no new runtime candidate, performance measurement, GC validation or conformance run in R19. Earlier identical-binary A/A measurements were already flat; timing these identical artifacts again would not distinguish a rebuild effect. Existing R18 behavioral and GC evidence remains attached to R18, with its original limitations. The no-regression requirement and the full parse/stringify, consumption, access, rotating-input, retained-output, short-call and options workload scope remain unchanged.

Exploratory inspection of R18's main and candidate workers found identical instruction counts and prologue stack sizes in eight selected functions: the parse entry, slow parse, direct array/object/number/string parsers, and JSON object/string constructors. Address and resolved-branch normalization still leaves differences, including data addresses and linker stubs. This is **not** a complete relocation-aware equivalence proof or evidence that code placement caused the measured slowdown. Full disassemblies and partial-normalization diffs are preserved without suppressing the remaining differences.

[Machine-code observations](results/rebuild-control-r19-validation/r18-core-machine.json), [partial normalization and limits](results/rebuild-control-r19-validation/r18-core-branch-normalization.json), [R18 measured rejection](https://github.com/PerryTS/perry/blob/44426f53184029e55ece042e6b077f3e66b0eabf/benchmarks/json_performance/TAPE_BATCH_R18.md).

The next implementation should remove demonstrated construction work: combine cached values with the uncached remainder during full lazy materialization, preserving aliases and mutations while avoiding subtrees that would immediately be overwritten. Admission thresholds and GC policy should stay unchanged for that experiment. The recurring direct-object control remains a required rejection test.

Script lint passes 73/74 checks, with the existing public-benchmark freshness failure. The file-size gate passes. The compile tier and two CI-only checks were skipped; full CI is not claimed. [Check results](results/rebuild-control-r19-validation/lint-results.json).

This branch contains diagnostic evidence only, with no runtime PR or release bump. [Artifact manifest](results/rebuild-control-r19-validation/manifest.json).

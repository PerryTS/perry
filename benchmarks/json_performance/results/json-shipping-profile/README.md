# JSON comparison with default release settings

**The array-prefix candidate remains rejected and reverted.** With default
release settings it improves large parse/stringify round-trip total CPU by
2.2–2.9%, including final collection. However, a seven-group ABBA repeat confirms
a 1 MiB record-object parse regression in every group: +0.22% to +0.73%, median
**+0.45%**. Retaining four large parsed graphs adds **2.265625 MiB peak RSS**.
The all-row no-regression objective remains open. Runtime source is still
`0327b9460a749592deb354d72ebad29bf4ae4bba`; nothing was pushed or merged.

The previous experiments used explicit runtime/stdlib codegen-unit overrides of
16 for iteration. This comparison rebuilds both the retained source and the
same [array-prefix patch](../json-prefix-retirement/README.md) with the default
`cargo build --release -p perry-runtime-static -p perry-stdlib-static` settings.
The effective package profiles are checked against `dist`: both runtime and
stdlib use one codegen unit, while the static wrappers remain at 16. Cargo
features, other fingerprint settings and both application objects match between
arms. No production profile, linker flag or GC policy was changed.

The executable text section shrinks from 13,503,904 to 11,496,928 bytes when
rebuilding the retained source with default settings. The default-profile
candidate has exactly the same total text size as its reference. Text size is
not a CPU or RSS measurement. Changing the build is not uniformly beneficial:
compared with the identical retained source at 16 units, default settings reduce
null parse CPU 11.36% and heterogeneous stringify 6.01%, but increase escaped
string parse 20.25% and 20 MiB record-object parse 3.23%. The escaped-string row
retires only 2.47% more instructions. These are measured profile effects, not a
claim about a particular cache or instruction-scheduling mechanism.

The numeric-array regression that rejected the candidate at 16 units does not
repeat with default settings: the full-run change is +0.16%, and the focused
median pair is negative. The earlier empty-object stringify increase also
reverses in the repeat. The record-object parse increase persists, so a build
change has not made the candidate meet acceptance.

The fresh Node/Bun comparison uses Node 26.5.1, Bun 1.3.14 and the same benchmark
Mac, fixture bytes, per-row iteration counts and warm-ups. All five engines are
randomized within each timing repeat. The `parent` label in raw data means the
retained source at 16 codegen units; `checkpoint` means that source at default
release settings; `candidate` means default settings plus the experimental patch.
It does **not** mean the older construction-batch source used by prior reports.

| Workload | Perry reference | Node | Bun | Perry / fastest |
|---|---:|---:|---:|---:|
| Tiny-object parse | 0.123 µs | 0.082 µs | 0.045 µs | 2.73× |
| Small-record stringify | 0.284 µs | 0.108 µs | 0.121 µs | 2.63× |
| 1 MiB escaped-string parse | 2.451 ms | 1.734 ms | 2.085 ms | 1.41× |
| 50,000-property object parse | 15.138 ms | 4.961 ms | 4.162 ms | 3.64× |

See **[all 38 CPU/RSS rows](measurements/recheck/all-38.md)** and the
[CSV](measurements/recheck/all-38.csv). The default-profile reference is at or
below both competitors' CPU medians in **15/38 rows**. It is at or below the
other engine's peak-RSS median in **67/76 pairwise comparisons**, and at or below
both engines' retained post-loop RSS in **33/36 cases**. These counts describe
this sample and its stated workloads, not statistical equivalence or a new
qualification of the older 147-case matrix.

The candidate's 36 main retained-memory comparisons span −0.0625 to +0.078125 MiB
peak-RSS changes. The supplemental large retained cases are more consequential:
four retained parsed graphs use 3.57% less total CPU but 2.265625 MiB more peak
RSS; 24 latest-only parses use 0.58% less CPU and 2.875 MiB less peak RSS; eight
retained stringify outputs use 0.34% less CPU and 0.03125 MiB more peak RSS.
No claim of memory neutrality or no regression is made.

Both default-profile binaries pass 40 compiled Node comparisons and 52 seeded
moving-GC runs each. The inherited root-array `Object.prototype.toJSON` mismatch
remains explicitly reference-compared. The 3,278 runtime unit tests, including
the strengthened 50,000-record edge-scan check, were run on this candidate source
in the preceding investigation; they were not rerun as release-profile unit tests.

The full window has 190 output checks, 950 timing trials, 540 retained-memory
trials and 108 lifetime trials, with 83 clean observations. The follow-up has
112 timing trials, 18 large retained trials and 15 clean observations. Both
entry/end gates pass, every retained output is checked after collection, and
none of these performance trials enables GC tracing or a debugger.

`prepare.py`, the saved workspace/config files, 90-file source stamps,
fingerprints, archive and worker hashes, raw results and validation are included.
Pinned private archives are `ship-checkpoint-runtime` and `ship-prefix-runtime`;
the latter is rejected. The shared build target currently contains that rejected
candidate, so it must not be mistaken for the restored source. No merge or push
has occurred.

The next investigation should profile the shipping build's escaped-string and
object parsing paths and examine how to avoid temporary array growth allocations
and copies. The modest prefix-retirement gain does not yet justify its tradeoffs.

Run `python3 benchmarks/json_performance/results/json-shipping-profile/verify.py`
to validate the restored source, build settings and recorded evidence.

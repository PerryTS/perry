# JSON code layout and direct-output investigation

**Both runtime experiments were rejected and reverted.** Runtime source remains
`0327b9460a749592deb354d72ebad29bf4ae4bba`; its bounded GC deferral and previously
measured benefits remain unchanged. This checkpoint adds reproducible evidence,
not a new runtime optimization. The original CPU/RSS and semantic requirements
are still open.

The linker control establishes that native code placement explains much of the
previous stack-plan experiment's unrelated regressions. The same pinned
application object and runtime archives were relinked with an order file derived
from the checkpoint's text symbols. No source, allocation policy or GC setting
changed between each original/ordered pair.

| Workload | Stack-plan CPU penalty, original order | With common requested order |
|---|---:|---:|
| 1 MiB record-array parse | +2.65% | +0.56% |
| 8 MiB record-array parse | +2.39% | +0.68% |
| Heterogeneous parse | +2.74% | +0.31% |
| Numeric-array stringify | +2.31% | −0.16% |

Seven randomized quadruples per case passed the host gate and continuous
observation. See [all eight comparisons](layout/table.md). Linker ordering is a
diagnostic here; no order file was added to Perry's production linker. The
checkpoint link applied 19,560 of 20,315 order-file entries. Duplicate/aliased
symbols, linker constraints and changed function sizes prevent identical full
layouts. This demonstrates a placement effect, not a particular cache or branch
predictor mechanism, and does not establish that arbitrary applications benefit
from this specific order.

The first new experiment shared object admission between the four-field scalar
serializer and the eight-field record serializer. It removes repeated receiver,
header, slot and key checks. Primitive-array fallback occurs before allocation;
plans still contain only lengths, indexes and inline text, and the input parent
remains rooted across the final output allocation. All GC policy remains unchanged.

Small-record stringify improved **2.26% CPU / 2.52% retired instructions**. In the
separate lifetime worker it improved **3.42–5.08% total CPU**, including final
cleanup. However, tiny-object stringify regressed **0.76%**, escaped-string
stringify **1.36%**, and the 24-call 20 MiB round trip **8.60%**. The last result
was consistent across the three fresh processes. Those costs disqualify the
change despite the targeted gain. See [all 38 rows](admission/measurements/table.md)
and [all 12 lifetime cases](admission/lifetimes/table.md).

The second experiment also outlined the general serializer. The direct entry's
native frame shrank from **272 to 96 bytes** and its static instruction count
from **1,846 to 518**; this count excludes the separately outlined function and
is not the number of instructions executed per call. Null stringify improved
**4.74%**, and small-record stringify **1.73%**. Tiny-object stringify worsened
**1.62%**, escaped-string stringify **1.51%**, and null/string/empty parse worsened
**9.07% / 7.72% / 3.06%**. The null and string parse rows retire nearly identical
instruction counts, again pointing to execution effects beyond source-level
work. See [all 38 rows](direct-entry/measurements/table.md). Its separate lifetime
run is **unqualified**: the local observer failed with ENOSPC, its observation
file is incomplete, and the end gate did not pass. Raw results are preserved and
labelled; they are not used to accept or reject the optimization.

Neither variant provides a meaningful RAM reduction. Across the 36 retained
memory comparisons, peak-RSS median changes span −0.031 to +0.141 MiB for shared
admission and −0.062 to +0.094 MiB for the outlined entry. Memory and instruction
counts do not erase CPU regressions. The qualified wide-object parse result
stays close to the checkpoint; its earlier roughly 26% API-loop CPU benefit
is preserved by retaining the original source.

A final, separate diagnostic makes the large round-trip failure more concrete.
Two traced runs of each binary, with a complete observer on the benchmark host,
give exactly the same collection counts in both repetitions:

| Runtime | Full collections | Minor collections | Total |
|---|---:|---:|---:|
| Retained checkpoint | 19 | 6 | 25 |
| Shared admission | 26 | 9 | 35 |
| Shared admission plus outlined entry | 26 | 9 | 35 |

These totals include process setup and the explicit cleanup calls. The traces
also show differing conservative native-stack root observations and differing
live-allocation/debt trajectories, despite unchanged GC-policy source. They do
**not** prove which particular root, allocation or frame change initiates the
divergence. Summed conservative-root observations are not unique retained
objects and also reflect the different number of collections. Instrumented
CPU/pause times are excluded from performance acceptance. The raw traces and
compact event sequences are under [trace-lifetime](trace-lifetime/summary.json).

The [subsequent root investigation](../json-root-retention/README.md) identifies
the first material retention difference: the checkpoint finds a stale pointer
to the previous call's large array in its trigger frame. Its extra retention
delays later collections. Debugger interventions establish this fixture's
mechanism but do not provide the general root proof needed to remove a scan.
Separately, further CPU micro-optimizations need layout controls and validation
under the actual shipping build profile. This series deliberately preserves the
existing matched 16-codegen-unit runtime/stdlib comparison profile; it does not
requalify the default shipping profile or establish a new Node/Bun ranking.

Each candidate passed 3,278 runtime tests (four ignored), 40 compiled Node
comparisons and 52 moving-GC runs. Two of those moving runs compare against the
reference with the previously documented inherited Node mismatch. Each full
measurement has 190 output checks, 570 timing trials and 324 retained-memory
trials. Shared admission's full/lifetime runs have 32/27 clean observations;
the outlined entry's full run has 33. Node-version and root-holder checks pass.
The inherited file-size and address-classification lint failures remain on
unchanged files; no new failure is hidden by a baseline update.

Patches, source stamps, binary hashes, build settings, validation and raw results
are preserved. Apply either candidate patch to the retained source in a separate
checkout to reconstruct it. The private release target currently contains the
**rejected** `defer6` build; use the separately pinned `defer2-runtime` archives
for the retained checkpoint. Rebuildable local test executables/caches were
cleared after recording evidence; object files, archives and worker binaries
were preserved. Nothing was pushed, merged or published.

Run `python3 benchmarks/json_performance/results/json-layout-and-admission/verify.py`
to verify the retained source and recorded evidence, including the explicit
qualification failure.

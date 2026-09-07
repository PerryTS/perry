# JSON string counting investigation

**The one-pass counter is preserved on the experimental branch
`codex/json-utf16-count-1520`. It has not met the no-regression requirement and
has not been pushed or merged.**

The active objective remains parity with both Node and Bun across parse and
stringify CPU/RSS workloads, including the earlier matrix. This report does not
claim that objective is met. The retained default-release reference is the
runtime at `0327b9460a749592deb354d72ebad29bf4ae4bba`.

## What the profiles established

Native three-second samples of the retained release worker put 717 of 2,269
Unicode-parse samples in the UTF-8 validator and another 166 in its UTF-16
counter. Escaped parsing has 1,445 leaf samples in string decoding, 581 in the
nesting-depth preflight, and 145 in builder finalization. These are sampled
stacks, not per-call CPU measurements. Both fixtures are objects containing
long strings, not scalar-root strings.

Separate fixed-count traces show 40 full collections for Unicode parsing and
one for escaped parsing with both release profiles. The old 16-unit build and
default one-unit build differ in traced pointer counts for Unicode. Matching
collection counts alone do not prove matching collection cost.

## Basic-validator experiment: not retained

The `string10` candidate replaced the compat validator with its basic variant,
which reports validity without locating the first error. This changes only the
UTF-16 counter, preserves its bounded WTF-8 fallback, and changes no GC policy.
All 3,278 unit tests, 40 compiled Node comparisons and 52 seeded moving-GC runs
pass; the inherited root-array prototype `toJSON` mismatch remains explicitly
compared with the reference.

A full default-release run has 152 output checks, 760 timing trials, 432 retained
memory trials and 108 lifetime trials. All finish successfully, entry/end quiet
gates pass, and the continuous monitor records 64 clean observations. Unicode
parse CPU changes from 255.599 to 253.090 microseconds per call (-0.98%), with
overlapping observed ranges. Unicode stringify changes +0.86%, also overlapping.
This is insufficient evidence of a meaningful end-to-end improvement. The
24-iteration large round-trip median has -8.36% total CPU but +14.1875 MiB peak
RSS; these lifetime figures include explicit final collection and are a
separate workload. No memory-neutrality or no-regression acceptance is claimed.

The isolated counter probe also shows why the basic API is not an adequate
solution: for an early malformed byte it scans the whole input before taking
the existing scalar fallback. Full Unicode validation remains far more work
than the counting result requires.

## One-pass counter

`string11` counts UTF-16 units on ARM while checking only the byte-shape property
needed to match the existing bounded scalar walker. It does not assert that
bytes are valid UTF-8 and never constructs an unchecked `str`. SIMD counts
non-continuation bytes plus an extra unit for four-byte leads. If a lead would
cause the scalar walker to skip a non-continuation, the existing scalar counter
handles the entire input. Stray continuation bytes, lone surrogates, overlong
forms and truncated final sequences retain their prior counts.

The counter checks complete 64-byte blocks, carries sequence requirements across
vector boundaries, handles ASCII blocks directly, and checks the vector/tail
boundary before scalar tail counting. It allocates no memory and adds no GC
hooks, roots, object bookkeeping or scheduling changes. Other architectures keep
the existing validator and counter.

The standalone prototype with the ASCII shortcut reduces median counter wall
time by about 34% for dense Unicode and 36% for mostly-ASCII input. These are
balanced fresh-process counter probes, not whole-JSON CPU/RSS qualification.
Its first version lacked the ASCII shortcut and was slower on ASCII-heavy
inputs; that version was not integrated. Both prototype versions pass an
independent scalar comparison over all Unicode scalars, arbitrary bytes, byte
mutations and truncations. Runtime qualification adds a test for arbitrary byte
shapes and ASCII/vector/tail transitions; all 3,279 runtime tests pass.

## Other rejected probes

Two ARM scanner variants packed SIMD match lanes into a scalar nibble mask.
Both were much slower in their standalone quote/backslash-scanning probe and
neither entered the runtime. These probes used stable rustc 1.97.1, unlike the
runtime's nightly 2026-08-20 build; they are limited diagnostic evidence, not a
shipping-profile JSON regression claim. Their source, compiler metadata and raw
results are preserved. The fused-counter probes use the pinned nightly compiler.

## Release measurements and the path control

The first `string11` four-engine window passes 152 output checks, 760 timings,
432 memory trials and 108 lifetimes, with 63 clean monitor observations. Unicode
parse CPU falls 13.09%, from 254.420 to 221.118 microseconds per call. A seven-group
paired repeat shows -12.93% to -14.27%, median -13.63%. The separate retained
Unicode parse lifetimes improve 11.37% to 12.09%. The first full-run peak RSS is
54.297 MiB for Perry, versus 166.938 MiB for Node and 159.781 MiB for Bun. In that
same window Node uses 437.647 microseconds and Bun 60.279: the candidate is about
1.98 times as fast as Node and 3.67 times slower than Bun. These numbers describe
this run, not a completed parity claim.

The first window also shows +0.37% empty-object parse CPU and +0.26% small-record
stringify. The repeat confirms an empty-object increase in all seven groups,
median +0.48%. Small-record stringify is positive in six of seven groups;
Unicode and numeric stringify changes reverse in the repeat. These comparisons
used distinct candidate/reference executable names and therefore also different
managed process arguments.

The 24-round-trip lifetime changes from +9.11% CPU in the first window to -8.07%
in the repeat, despite unchanged binaries. The repeat also has +14.1875 MiB peak
RSS. This prompted a direct control instead of attributing the change to the
counter or GC policy.

With the same reference binary invoked through two different paths, six samples
per path show median total CPU 3.322 versus 3.615 seconds, an approximately 8.8%
difference. Process-wide instruction counts differ too. Copying each arm to a
single fixed executable pathname reduces their median difference to +0.165%
and gives identical median peak RSS (276,807,680 bytes). There are ten samples
per arm in this fixed-path control. It proves that invocation path is a material
confounder for this lifetime probe; it does not yet identify the runtime cause.

Separate traced executions at the common path have identical counts: 37
collections, 28 full collections and 36,210,217 pointer slots read. Traced runs
through the two original paths also have 37/28 collection counts, so these
traces do not explain the untraced CPU difference. Tracing must not be treated
as a transparent observation of this effect. The local diagnostic similarly
has 37/28 for all three binaries. `gc_check_trigger` has the same disassembled
instructions after normalizing symbolic call relocations; this does not prove
that every caller or conservative root is the same.

The path-control window includes 32 untraced trials and four separate diagnostic
traces, with 31 clean monitor observations and successful quiet gates. The final
all-38 comparison uses one executable path for both Perry builds, verifies the
copied binary hash before every trial, and records that path in every result.
That corrected comparison is complete: 152 output checks, 380 Perry timing
trials, 216 retained-memory trials and 108 lifetime trials, with 58 clean monitor
observations and successful entry/end gates. All measured Perry trials record
the same executable path. Node and Bun serve as output oracles in this window;
their timing figures above come from the separate four-engine window.

See [all 38 rows with a common invocation path](string11/fixed/results/recheck/all-38.md)
and [the earlier four-engine CPU/RSS table](string11/performance/results/recheck/all-38.md).
The corrected main Unicode parse median is 258.814 to 238.553 microseconds
(-7.83%), with separated observed ranges. The Unicode parse lifetime medians
improve 11.33%, 13.00% and 12.17% for discard, latest and retain respectively.
Their peak-RSS changes are zero, -0.015625 MiB and -0.03125 MiB. Large round-trip
lifetime CPU changes are +0.14% to +0.22%, with identical median peak RSS at all
five iteration counts.

Several other timing rows still have broad overlapping ranges. The largest
positive main-loop medians are +13.77% for 16 KiB record-array stringify and
+9.02% for long ASCII string parse; neither has separated ranges. These figures
must remain visible: overlap does not establish equivalence, and a useful
Unicode gain does not establish no regression elsewhere. All 36 main retained-memory median peak-RSS changes are between -0.078125
MiB and zero, as recorded in the memory summary. The candidate
is therefore preserved for continued work, with **no no-regression acceptance**.

## Validation and next work

The runtime change is limited to the UTF-16 counter, its ARM helper and its
property test. It adds no allocations or GC behavior. The matched release
binaries pass 40 compiled Node comparisons and 52 seeded moving-GC checks each;
the 3,279-unit-test result is from the standard runtime test profile. The
inherited prototype-toJSON mismatch is still explicitly reference-compared.
The root-holder inventory passes. The existing file-size and address-inventory
failures are recorded; no baselines were relaxed and no new violations were
introduced.

Source stamps, both runtime/static-wrapper build settings, object/archive/worker
hashes, drivers, raw results, diagnostics and the candidate patch are included.
The default release profile matches `dist`; no compiler profile was changed.
The original GC branch remains at `ae1f89611`. Historical reports retain their
original checkpoint and source-pinned verifiers; this report's verifier checks
the current experimental counter instead.

The next measurement change is to use an immutable executable inode at the
common pathname, avoiding file-content rewriting between trials. That control
has not yet been tested and is not asserted to explain the remaining variance.
Then repeat the uncertain rows before any acceptance decision. The next decoder
work should remove per-byte output-buffer checks and repeated scans in escaped
strings while preserving strict JSON syntax and WTF-8/surrogate behavior.
The earlier 147-case matrix and full Node/Bun parity remain open.

Run `python3 benchmarks/json_performance/results/json-string-counting/verify.py`
to check this report and the current experimental source.

# Retiring unpublished JSON array growth buffers

**The candidate was measured, rejected and reverted.** Runtime source remains
`0327b9460a749592deb354d72ebad29bf4ae4bba`. The full 38-row run initially showed
no CPU increase with disjoint observed ranges, but the focused repeat confirmed
a numeric-array parse regression in every one of seven randomized ABBA groups:
**+1.49% to +1.72%, median +1.55%**. That disqualifies the build despite its
large round-trip improvement. No new all-row parity or no-regression acceptance
is claimed, and nothing was pushed or merged.

The [root investigation](../json-root-retention/README.md) found dead old arrays
keeping young JSON records alive. Some of those arrays are unpublished growth
buffers: the parser copies their elements into larger buffers, yet installs
layouts and remembered edges on each abandoned prefix. Those duplicate edges
make a minor scan the same children repeatedly.

The candidate removes that intermediate publication. After copying an exclusively
owned buffer, it sets the old length to zero and clears pointer-bearing payload
words, including addresses a diagnostic whole-payload scan could otherwise find
in slack. The final array still receives its complete layout and remembered
edges. Collection remains suppressed throughout construction; batch admission
still refuses active incremental marking. GC scheduling, allocation generations,
roots, barriers and the ordinary non-batched growth path are unchanged.

A local trace of the 24-call large round trip gives:

| Work | Checkpoint | Candidate |
|---|---:|---:|
| Full / minor collections | 19 / 6 | 19 / 6 |
| Remembered slots scanned | 2,344,560 | 870,000 |
| All pointer slots read | 32,912,743 | 29,963,623 |
| Objects promoted | 3,485,192 | 3,485,192 |

The duplicate slot reduction is 62.9%; total slot reads fall 9.0%. Promotion
does not fall because the final old array still holds the children. These local
traces are mechanism evidence only; traced timing is excluded from acceptance.

The qualified benchmark-host lifetime comparisons include final cleanup CPU:

| Large round-trip calls | Checkpoint ms | Candidate ms | Change |
|---:|---:|---:|---:|
| 7 | 1,126.036 | 1,106.052 | −1.77% |
| 8 | 1,289.292 | 1,269.980 | −1.50% |
| 9 | 1,386.144 | 1,367.471 | −1.35% |
| 16 | 2,429.939 | 2,397.722 | −1.33% |
| 24 | 3,381.813 | 3,343.611 | −1.13% |

See [all 38 rows](measurements/recheck/table.md),
[all 12 lifetime cases](measurements/lifetimes/table.md), and the
[focused paired changes](followup/results/focused/table.md). The numeric row's
initial retired-instruction median changes only +0.02% while loop CPU rises
1.45%; the repeated CPU increase is 1.55%. Its default parse uses the lazy tape
route, outside the changed construction helper. Native placement is a plausible
explanation, supported by the prior controlled layout experiment, but this
candidate has not itself had a common-order or shipping-profile comparison.
Unicode stringify also warrants attention: its focused median pair is +1.21%,
with one negative pair and six positive pairs.

Memory does not materially improve. Across the 36 retained-memory comparisons,
peak-RSS median changes span −0.0625 to +0.078125 MiB, and post-loop RSS changes
span −0.125 to +0.125 MiB. Some increases of one to three 16 KiB pages repeat
with separated ranges; they are reported rather than called zero. Additional
large retained cases verify output after collection: retaining four parsed
graphs changes total CPU −1.30%, keeping only the latest of 24 parses −0.24%,
and retaining eight stringify outputs approximately 0.00%. Their peak-RSS
changes are −0.0625, +0.0625 and +0.015625 MiB respectively.

The candidate passes 3,278 runtime tests (four ignored), 40 compiled Node
comparisons and 52 seeded moving-GC runs. The 50,000-record moving test is
strengthened to bound remembered-slot work near the final array's 50,000 edges
while proving the child actually moves and the complete output survives.
The known inherited root-array `Object.prototype.toJSON` mismatch remains
reference-compared; no baseline was updated to hide it. Node-version and
root-holder checks pass. File-size and address-classification failures are the
same unchanged files documented in the preceding report.

The main measurement has 190 output checks, 570 timing trials, 324 retained-memory
trials and 108 lifetime trials, with 64 clean host observations. The final
supplement has 224 timing trials, 18 large retained trials and 25 clean
observations; both entry/end gates pass. Its first attempt is explicitly
unqualified: the supplemental harness requested round-trip retention, but the
pinned worker always discards round-trip outputs. Output verification stopped
the run and no end gate was reached. The corrected run uses supported parse
and stringify retention modes. Both attempts are preserved.

These builds use the existing matched runtime/stdlib 16-codegen-unit comparison
profile, with unchanged application objects and verified Cargo features. The
next investigation rebuilds both arms with the default release settings, which
the manifest defines to mirror `dist`, before making any shipping claim. The
default runtime and stdlib codegen-unit count is one; wrapper crates remain at
16. No production profile or linker flags were edited.

Source stamps, candidate patch, binary/archive hashes, validation logs, raw
measurements and debugger-independent traces are preserved here. The private
`prefix8-runtime` archives reconstruct the rejected candidate; the pinned
`defer2-runtime` archives remain the retained comparison checkpoint. A target
directory being rebuilt for the next experiment is not an immutable reference.

Run `python3 benchmarks/json_performance/results/json-prefix-retirement/verify.py`
to verify this evidence and the restored checkpoint source.

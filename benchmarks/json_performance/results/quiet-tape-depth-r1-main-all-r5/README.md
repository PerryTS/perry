# Tape depth R1 versus freshly built merged main

Quiet M1 window 2026-09-09 22:10:10–22:15:21 UTC. Candidate build/source
`f55569a565581fc964a73b063213774b4601d4e1`, version 0.5.1530; reference build
`eee3881c464bf91ae900e87a42bed072bbdfc95a`, version 0.5.1529. Node 26.5.1 and
Bun 1.3.14. Five repetitions with equal work, default GC and frozen worker objects
linked against matched runtime archives. [Build and GC validation](../tape-depth-r1-validation/README.md).

All 200 output checks and 344 measurement groups validate. Candidate retains
46/50 CPU, 67/86 peak-RSS and 31/36 retained-current-RSS leads against the better
Node/Bun median. Remaining parity gaps are visible in [all targets](parity.md);
this does not establish completion of the parity goal.

Eligible record-array parse medians improve 16.26% at 13 KiB, 15.01% at 1 MiB,
and 14.39% at 8 MiB versus main. Full scans improve 4.62%, 5.96% and 5.89%,
respectively. Sparse access improves 11.54%, 14.91% and 13.66%. Untouched
roundtrip improves 11.37%, 11.27% and 9.82%. The 20 MiB array uses the direct
route and its parse/roundtrip medians move +0.06%/+0.12%.

No CPU row has fully separated slower ranges. That is not proof of equality:
ASCII/Unicode parse are +0.89%/+1.13%, wide-object parse +0.14%, and other small
positive deltas are retained. ASCII/Unicode stringify are -6.96%/-4.32% in this
window, unlike the +7.03% Unicode concern in the longer-count focused replay.
Do not dismiss or select away that conflicting evidence. Further evaluation is
required before readiness. Maximum median peak/current RSS increases across all
rows are 160/96 KiB; this does not address large-container memory gaps to Bun.

[Every CPU, peak and current RSS row](comparison.md), raw JSONL, exact runners,
quiet window, source patch, worker/source hashes and GC witnesses are archived.
`reference-screen.json` preserves all candidate/main deltas. Repeated-source
parsing may use existing caches; changing-input results are measured separately.

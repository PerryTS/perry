# Opening scan R3 versus fresh merged main: original suite

Quiet M1/8 GiB window 2026-09-09 20:46:07–20:51:14 UTC. Four engines, five
repetitions, 200 passing correctness checks and 344 validated measurement groups.
Candidate is the immutable R3 build; baseline is freshly built merged main
`eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529). Both build provenances
are archived here. The same original workload object is linked in both arms.

Candidate leads 46/50 CPU, 67/86 peak-RSS and 31/36 retained-current-RSS rows
against the better Node/Bun median. Existing full-scan, large-roundtrip and
memory gaps remain. This worker repeats one input; the changing-input suite
measures the separate eight-source case.

No timing row has separated slower candidate/reference sample ranges. Empty,
tiny, small and 1 KB object parse medians improve 2.11%, 1.47%, 1.10% and 1.22%.
However, sparse 13 KB reads are 0.50% slower and heterogeneous 1 MB parsing is
0.23% slower by median, with overlapping samples. Large ASCII/Unicode stringify
medians rise 5.43%/3.92% in variable, overlapping samples. The [longer replay](../quiet-opening-scan-r3-main-regression-r9/README.md)
retains positive sparse/heterogeneous and variable large-string medians;
overlapping ranges alone do not prove equality.
Maximum median peak/current RSS increases are 192/144 KiB.

[Complete CPU/RSS comparisons](comparison.md), [Node/Bun target rows](parity.md),
[all reference samples](reference-screen.json), and [quiet admission](window.json)
are archived with exact runners, raw trials, source patch and GC witnesses.

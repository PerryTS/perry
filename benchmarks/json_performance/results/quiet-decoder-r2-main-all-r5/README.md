# Complete R2 / merged-main parse and stringify comparison

Four engines, five repetitions, 200 output checks and 344 measurement groups.
The quiet window, exact runner, source patch and worker hashes are preserved.
`baseline` is the freshly rebuilt e7223f700 main runtime (0.5.1528); `perry` is
PR10034 decoder R2 (0.5.1529). Node26.5.1 and Bun1.3.14 run on the same M1/8GiB
host with equal work. Correctness and process exit codes are checked separately
from timing. Repeated-source parses can use existing source reuse; see the
changing-input matrix before drawing conclusions about fresh object parsing.

Both main and R2 lead 46/50 CPU, 67/86 peak RSS and 31/36 retained-current RSS
medians. The largest slower R2 CPU median is +0.315% (8MiB envelope stringify),
with overlapping samples; no R2 CPU row has all its samples above all main
samples. Small-array sparse reads are +0.042%, consistent with the longer paired
trial (+0.063%) after the R1 regression was removed. CPU medians alone do not
establish significance. Process RSS differences are at most 0.078125 MiB (80 KiB).

All rows, including unfavorable ones, appear in comparison.md. reference-screen.json
contains every candidate/main delta and both CPU sample lists. parity.md reports
R2 against Node/Bun; merged-main-standings.json reports main's own counts and gaps.

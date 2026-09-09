# Source-length R4: longer paired small-object checks

Quiet M1 window 2026-09-09 19:16:58–19:18:18 UTC. Nine trials per arm,
three million changing-input parses per trial after 5000 warmups. Three arms:
R4 candidate, corrected R2 baseline, and prior source-length R3. Eight Node /
Perry output checks and all 54 timed checksums pass. All raw trials, executable
hashes, imported runner, admission evidence, source patch and GC proof survive.

Small-record CPU medians: R4 0.492515 us, baseline 0.491678 us, R3 0.493604 us.
R4 is +0.1703% versus baseline and -0.2206% versus R3. Samples overlap. The other
small-object row is -0.1770% versus baseline, also overlapping. Peak RSS differs
by at most 48 KiB. These matched longer trials reduce the R3 cost but leave the
small-record median higher a second time; they do not establish significance
from medians alone or prove a regression-free implementation.

R4 remains experimental under the requested no-regression condition. No full
matrix is claimed. The large-string gains remain established by the separate
four-fixture focused run. Next, test the opening-byte preflight improvement as
an independent candidate on the corrected R2 baseline, then revisit this source
length shortcut alongside the remaining small-record construction work.

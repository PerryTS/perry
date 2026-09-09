# Fresh merged-main reference check

Matched release build of main e7223f700c8ce69c388210dab394ad7142550527
(v0.5.1528). Workers relink the same immutable workload objects against this
fresh runtime. The compiler and both static wrappers were built together; their
hashes and the clean tracked source tree are recorded in provenance.json.

In this run `perry` means freshly built main, `baseline` means the earlier
b211 PR-head reference, and `prior` means decoder correction R1. All nine
Perry/reference output comparisons against Node and 81 timing trials passed;
the saved admission window passed the quiet gate. Three cases, nine repeats,
randomized three-arm order, fixed work counts.

| Small record array | Main CPU us | Earlier reference us | R1 correction us | Main / earlier delta | R1 / main delta |
|---|---:|---:|---:|---:|---:|
| parse | 18.14568 | 18.13378 | 18.17000 | +0.066% | +0.134% |
| sparse | 25.09836 | 25.08104 | 25.73310 | +0.069% | +2.529% |
| scan | 77.44560 | 77.47360 | 77.73010 | -0.036% | +0.367% |

The rebuilt merge closely tracks the earlier reference on these cases. The R1
sparse regression remains, so switching the reference does not explain it away.
This is a focused check; full Node/Bun standings require the full matrices.

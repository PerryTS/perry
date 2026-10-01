Node-API sidecar verifier and staging microbenchmarks on macOS ARM64.

The large-payload arms ran five fresh processes each, with 40 operations per
process and interleaved arm order; numbers are medians. CPU is user + system time reported by `/usr/bin/time -l`. Concurrent
compiler builds were active, so wall time is deliberately omitted. Baseline is
main `9e29f59d43`; the harness compiles the exact baseline and patched function
bodies with the same SHA-256 dependency, without sharing prebuilt Perry binaries.
This isolates authentication/copying costs; it is not an end-to-end startup claim.

| Verification payload | CPU seconds | Peak RSS bytes |
| --- | ---: | ---: |
| Baseline, all 28 MiB | 0.75 | 35,405,824 |
| Streaming, same 28 MiB | 0.75 | 2,588,672 |
| Streaming, retained 2 MiB | 0.05 | 2,588,672 |

The synthetic audit-like payload is a 2 MiB selected addon, 10 MiB development
source and a 16 MiB foreign prebuild. The unchanged-payload control measures
streaming separately from pruning. Hashing still checks every selected byte;
there is no persistent verification cache. Run `python3
benchmarks/issue-10460/verifier.py` to repeat the verifier comparison.

A tiny-payload control ran seven interleaved processes per arm, each verifying
the same 16 KiB file 10,000 times. Median CPU decreased from 0.58 to 0.55 seconds.
Median RSS was 1,769,472 versus 1,802,240 bytes; process-level ranges overlapped
(1,736,704–1,818,624 versus 1,769,472–1,851,392 bytes), so the 32 KiB median
difference is not evidence of a payload memory change: both arms allocate
16 KiB. Repeat with `python3 benchmarks/issue-10460/verifier.py --tiny`.

The staging control keeps `fs::copy` in both arms, including macOS extended
attributes, then compares whole-file versus bounded streaming hashes:

| Single file staged 40 times | Baseline CPU / RSS | Streaming CPU / RSS |
| --- | ---: | ---: |
| 2 MiB | 0.10 s / 8,159,232 B | 0.09 s / 4,997,120 B |
| 16 MiB | 0.65 s / 21,757,952 B | 0.63 s / 4,997,120 B |

Real loading, relocation, dependent-library/data use, repeated-load identity,
permissions, extended attributes, and tamper rejection are exercised by the
unit and Node-API end-to-end regression tests. Runtime and staging hashing use
file-sized buffers capped at 256 KiB; deployment integrity is checked before the first library load.

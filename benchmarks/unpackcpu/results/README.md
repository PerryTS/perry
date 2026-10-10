# Measurement evidence

REPORT.md describes the combined change and its measurements. The raw interleaved rows, statistics, controls and profiles are kept outside the repository to keep it small. Larger perf dumps, binaries, test logs and fixtures remain on the Linux measurement host.

The harness records the owned Linux-lane paths and requires those external fixtures. Upstream upm sources and npm tarballs are read-only inputs and are not included. prepare_micro.py and backend_budget.py next to this directory reproduce the worker fixture and both regression witnesses.

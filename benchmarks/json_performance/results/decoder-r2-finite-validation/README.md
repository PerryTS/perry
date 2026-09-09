# Finite-checksum escaped-record replay

The immutable corrected R2 and rebuilt main workers are replayed through all
seven escaped-record fixtures in scan and sparse modes. The final fixture now
includes a numeric `id` without changing its duplicate or escaped keys.

All 70 processes return finite numeric RESULT fields and the expected checksums:
128 for scan, 2 for sparse. All 42 candidate comparisons (normal, scheduled /
protected, and full GC) match Node 26.5.1. Twelve of the fourteen main comparisons
still fail serialized-output equality, demonstrating the correction remains live.
These are correctness checks, not CPU/RSS measurements or new collection witnesses.

Worker, fixture-source, JS worker and driver hashes are pinned in metadata.json.
The source tree identifies the correction branch; the immutable worker hashes,
not the checkout version or current target archives, identify the tested binaries.
The output and fixtures are retained; the executable replay command is in
[the parent validation directory](../decoder-r2-validation/validate.py).
The driver-recorded.txt copy here preserves the executed source.

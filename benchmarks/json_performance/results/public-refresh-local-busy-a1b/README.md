# Local public refresh rejected before measurement

The canonical public-baseline refresh started at PR source a1b39840283ce7c771a776706621a9ca249a8e8e.
Its full five-package release build passed in 6m05s. CPU never met the required
<=25% active for 60 continuous seconds within the 15-minute admission deadline.
The runner exited 1 before the first suite. No timing component, replacement
public artifact, or generated public table was produced. Source and HEAD stayed
unchanged throughout the attempt. The initial/final process lists, a diagnostic
CPU sample, exact driver/configuration and logs are retained.

The preceding local landing-gate run passed 79 of 80 executable checks; two
CI-only commands were skipped. Its only failure was the public artifact's stale
source fingerprint, independently identical on current main and the PR. This
failed refresh does not fix that gate and is not performance evidence.

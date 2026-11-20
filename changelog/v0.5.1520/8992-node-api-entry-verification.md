Node-API sidecar loading now rejects an addon entry unless its canonical path
is one of the payload files whose size and SHA-256 were successfully verified.
This keeps the runtime authorization boundary self-contained even if sidecar
staging changes in the future.

The sidecar contract now also documents the accepted pathname-reopen TOCTOU
window and requires deployments to protect sidecars from concurrent writers.

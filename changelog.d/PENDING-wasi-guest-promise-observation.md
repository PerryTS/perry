Add nonblocking Promise observation to the rooted canonical guest frame C API.
Polling returns pending, a separately rooted fulfillment value, or the original
rejection in the frame's error slot. It marks rejection handled without running
jobs, invoking thenables or waiting for I/O. Invalid inputs remain inside the C
error boundary, and cleanup preserves an earlier error.

Keep guest adapter ABI 2 and the existing async guest generation guards. This
is a foundation for async adapters; task scheduling, canonical suspension and
cancellation remain unfinished. Add moving-GC, result ownership, rejection
identity and invalid-input tests. Update the WASI support matrix and correct
stale descriptions of the legacy and persistent host paths.

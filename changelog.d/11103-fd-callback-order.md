Fix the remaining scheduling race in the fd-mutator parity test (#11103).
The callback results were ordered relative to each other, but could still
interleave with the promise results. Await all three concurrent callbacks
before starting the promise cases or removing the fixture directory. All
EBADF/code/syscall assertions remain unchanged.

A pinned-Node probe that delays the first callback of each mutator reproduces
the original output mismatch without any Perry runtime involvement.

Programs no longer abort at startup on Linux kernels built without transparent
huge pages (#12863). On those kernels `madvise(MADV_HUGEPAGE)` and
`madvise(MADV_NOHUGEPAGE)` fail with `EINVAL`, and the arena asserted that both
succeed. THP advice is now best effort: `EINVAL` is ignored, and any other
error still fails the assertion.

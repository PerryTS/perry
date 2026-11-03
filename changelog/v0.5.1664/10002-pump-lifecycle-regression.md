### Fixed

- Verify native handle recycling through allocation-only hook registration, public extension registration, and the actual runtime pump, including same-tick re-entry and the next outer tick, in an isolated bounded test process.
- Check that overlapping outer guards share one lifecycle tick and that eager depth restoration releases only the current thread's contribution.
- Clarify that HTTP callbacks rely on the outer tick's quarantine promotion.

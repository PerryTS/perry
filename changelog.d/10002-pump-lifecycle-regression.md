### Fixed

- Verify native handle recycling through the public extension registration and actual runtime pump, including same-tick re-entry and the next outer tick, in an isolated bounded test process.
- Clarify that HTTP callbacks rely on the outer tick's quarantine promotion.

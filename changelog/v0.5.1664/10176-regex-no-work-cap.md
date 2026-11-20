### Fixed

- **RegExp operations are no longer capped by work** (#10164). Valid programs that Node completes threw `RangeError: Regular expression work limit exceeded`: a 32,000-unit non-ASCII `split`, a 60,000-unit global `replace`, and linear splits and replaces of 11–15 million units. No finite allowance separates valid programs from pathological ones, so a catastrophic pattern now runs as long as it does in Node. Matching still yields to the collector and to cancellation, and the memory limits are unchanged.

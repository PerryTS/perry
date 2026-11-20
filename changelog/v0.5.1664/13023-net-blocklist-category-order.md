### Fixed

- `net.BlockList` serializes rules as addresses, then subnet metadata, then ranges,
  regardless of category insertion order, preserving existing per-category order
  and duplicates. This does not solve general unordered-map address traversal or
  repeated/mapped-family address deduplication.
- Validation on the measured category-only candidate used a coherent production
  build and four self-verification calls, all exit 0 with raw Node 26.12.1-exact
  output. Independent paired proof recorded baseline exits 1/1/1/0 and candidate
  exits 0/0/0/0 for JSON validation, IPv4, IPv6, and membership checks respectively;
  every candidate output was raw-Node-exact. This is not full NET or latest-main
  certification, and does not claim Rust-test execution.

### Runtime

- Add an opt-in `PERRY_MEMORY_PROFILE=small` policy for Linux executables using
  mimalloc. Configure THP before startup allocations while preserving explicit
  allocator environment settings; document throughput and embedding tradeoffs.

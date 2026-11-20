**`JSON.parse` now handles deeply nested documents without exhausting a worker
thread's native stack.** Inputs beyond the recursive fast path use strict tape
validation and heap-backed iterative materialization, with a separate
500,000-level resource limit.

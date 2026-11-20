- **Complete `bun:ffi` callbacks (#6562).** `JSCallback` now exposes GC-rooted,
  same-thread C-ABI trampolines for scalar arguments and returns, and
  `FFIType.function` is accepted by `dlopen` symbol signatures. Callback
  wrappers provide Bun-compatible `ptr`, `threadsafe`, and idempotent `close()`
  members; cross-thread/threadsafe calls and uncaught callback exceptions fail
  closed without unwinding through native frames.

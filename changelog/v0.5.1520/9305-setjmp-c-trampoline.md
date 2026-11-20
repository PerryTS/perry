### Fixed

- `cc --help` (and any program throwing from inside a microtask) no longer
  segfaults: every jmp_buf the exception transport can `longjmp` to is now
  armed inside a dedicated C trampoline (`perry_sjlj_try`) instead of a raw
  `setjmp` call in Rust. rustc cannot express `returns_twice`, so a Rust
  frame containing a live `setjmp` was compiled under LLVM's one-return
  assumption — in `run_microtasks` LLVM colored the stack slot of the
  spilled TLS-base temporary into the task-record copy loop, and the
  longjmp return path reloaded NULL. The trampoline makes the hazard
  unrepresentable for all current and future Rust trap sites; the one
  remaining raw `setjmp` (the GC's register-snapshot spill, which never
  longjmps) is documented as the deliberate exception. (#9305)

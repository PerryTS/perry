- **perry/thread: a worker no longer reads a main-heap String or BigInt out of a shared module global.**
  perry/thread agents share user-module globals, but each agent owns its own moving heap. A worker
  that read a String/BigInt module global (directly, through an imported function, or through a class
  method) kept main's pointer; when main's collector evacuated the value, the worker's copy pointed at
  retired from-space, and under forced evacuation, from-space protection or the seeded GC schedule the
  program crashed (SIGSEGV/SIGBUS with a "retired from-space" report). For a binding with one
  initializer and no later writes whose value can be a String or BigInt, the owning initializer now
  publishes a pointer-free record once (String bytes with the exact UTF-16 length and lone-surrogate
  flag, or BigInt limbs), and each agent materializes an ordinary movable replica in its own heap on
  first read, rooted in a per-module thread-local block. A ready read is a state compare and a load
  from that block, with one TLS address per function invocation. Reads before the initializer runs
  stay `undefined` and retry; nothing blocks. Thread-free programs and programs that use
  `worker_threads` emit no new code. Not covered: module-global objects, arrays and closures read by
  perry/thread workers, and programs that mix `Worker` with perry/thread; those are tracked by the
  agent-local module-state design.

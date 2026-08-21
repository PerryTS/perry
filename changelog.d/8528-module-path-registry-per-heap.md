### Fixed

- **Runtime: the path-module registry is now per-heap, so one process can host
  several Perry applications.** `PathModuleEntry::exports` holds NaN-boxed
  pointers into the arena of the thread that produced them, and both the arena
  and the mutable-root scanners are thread-local. The registry was
  process-global, so a single `OnceLock<ThreadId>` had to fail closed for every
  later thread (`ERR_PERRY_PATH_MODULE_THREAD`) to stop one heap's pointer
  reaching another heap's collector — which meant a host could load at most one
  application that uses runtime path modules.

  The table conflated two facts with different lifetimes. A canonical path's
  `<prefix>__init` **address** is a property of the program: codegen registers it
  once, on whichever thread runs module init first, and it is a code pointer no
  collector cares about — so it stays process-wide. The **exports** and status
  are arena pointers, so they are per-heap. A heap that has never required a
  path now adopts the program-wide initializer rather than inventing an empty
  entry; making the whole table per-heap instead left later heaps silently
  resolving modules to an empty `module.exports`.

  Both the export table and the initializer-address map are per-heap. A
  process-wide address map was tried and measured wrong: a host that loads
  several application libraries gets its own copy of each module per library,
  at its own code address, under the SAME baked canonical path, so the map saw
  "same path, different address" and refused the second library as a duplicate.
  Two Next.js applications in one Coop process produced 115 such rejections,
  with the second application's modules never initializing (200 for the first,
  500 for the second).

  Keyed by thread rather than by `AgentId`, deliberately: `CURRENT_AGENT` is
  itself a `thread_local!` defaulting to `PRIMARY_AGENT`, and an embedder's app
  thread is a plain `std::thread::spawn` that never calls `enter_worker_agent()`,
  so agent-keying would hand every app in a host one shared table. The GC scanner
  drops its owner gate for the same reason — the collector running it owns every
  pointer in the table by construction — and uses `try_with` so a scanner running
  during an app thread's heap teardown cannot panic on a destroyed thread-local.

  Reachable only through Next.js chunk loading (`js_register_path_init` is
  emitted for `nextjs_path_init_modules` alone), so ordinary programs are
  unaffected.

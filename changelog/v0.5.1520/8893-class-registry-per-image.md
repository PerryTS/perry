### Fixed

- **Runtime: class registries are per image, so one process can host several
  Perry applications.** Every class-id-keyed table module init writes —
  `CLASS_VTABLE_REGISTRY` (instance methods / getters / setters),
  `CLASS_STATIC_METHODS`, `CLASS_STATIC_ACCESSORS`, `CLASS_CONSTRUCTORS` and
  their flags, the parent-edge map and its dense mirror, `CLASS_NAMES`,
  `CLASS_LENGTHS`, `REGISTERED_CLASS_IDS`, the bind-length tables, the
  `extends Error` / `DataView` / typed-array marks, the `Symbol.hasInstance` /
  `Symbol.toStringTag` hooks, the generic-origin and fetch-parent maps,
  `ANON_SHAPE_CLASS_IDS` — was a process-global `static` keyed by
  compile-time class id (#8546). Class ids are assigned by codegen from a small
  sequential counter, so N dlopen'd copies of an application register the SAME
  ids with DIFFERENT `func_ptr`s (each image's own code addresses), and
  `HashMap::insert` is last-writer-wins: after the last image's init, every
  class of every earlier image dispatched into the last image's code. In a Coop
  daemon hosting several Next.js deployments only the last-initialised one
  served; the others died on their first by-name resolution with
  `TypeError: value is not a function`. No write order over a shared table can
  work (first-wins for methods leaves every vtable a mix of two images;
  first-owner for every entry point leaves later images unable to initialise),
  so the tables are now per **image**.

  The model (`crates/perry-runtime/src/object/class_image.rs`): the tables live
  in one `ClassImageTables` per image, reached through a thread-local handle.
  `js_gc_init` — the first runtime call codegen emits in both `main` and
  `perry_module_init`, on the thread that runs that image's module init — gives
  the thread its own image (the first thread to enter owns the *primary*
  image; every later one gets a fresh image). `perry/thread` workers (`spawn`,
  `parallelMap`, `parallelFilter`) and `worker_threads` Workers adopt their
  spawner's image before they run anything, because they never run module init
  and must dispatch through the spawner's tables. A thread that neither entered
  nor adopted — a pump firing JS on the primary heap's behalf (Android's UI
  thread), a reactor thread, a libtest thread — reads and writes the primary
  image, which is exactly the process-global table it saw before; a program
  with one image is behaviourally unchanged. Keying by thread alone was
  rejected because those pump/worker threads run JS that dispatches through
  these tables without ever running init; keying by `AgentId` was rejected for
  #8528's reason (a host's app thread is a plain `std::thread::spawn` that
  never claims an agent). Each former `static RwLock<..>` is now a `static
  ImageTable<RwLock<..>>` whose `read()` / `write()` resolve the calling
  thread's image and return the same guard types, so the ~100 call sites are
  unchanged. The `RegistryLatch`es and `VTABLE_GEN` stay process-global on
  purpose: a latch armed by any image only ever costs another image the slow
  path, never a wrong answer.

  Regression tests (`object::class_image::tests`): two application threads
  register the same class id with different method addresses and each
  dispatches to its own (sabotage-verified: with `enter_current_thread_image`
  a no-op, the last writer wins and the test fails); a spawned worker shares
  its spawner's image while a second application sees neither; a thread with
  no image reads the primary. Cost: the dense parent-edge read
  (`get_parent_class_id`, the hottest class-registry read) now goes through
  the thread-local image resolution (a cached-TLS load) before the indexed
  atomic load, and each image allocates its 256 KiB dense table on the heap
  instead of sharing one `.bss` array. `CLASS_STATIC_ACCESSORS` leaves the
  `per_test_global!` set (the per-image handle already keeps one libtest
  thread's clear out of another's reach), and the GC test guards no longer
  clear it — it holds code addresses, not roots.

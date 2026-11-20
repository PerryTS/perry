### Performance

- **The two hottest side-table probes answer "no" from an inlined address
  compare instead of an out-of-line registry lookup, and `is_closure_ptr` tests
  the tag before the arena.**

  `is_registered_buffer` and `lookup_typed_array_kind` are asked "is this
  pointer special?" from ~239 and ~200 generic call sites — every property
  get/set, every prototype walk, every element read, every `[[HasProperty]]`.
  #9176 gave both a monotone "has anything ever been registered?" latch, which
  makes the answer free for a program that never allocates a `Buffer` or a typed
  array. `claude-code` allocates both, so the latch is armed and stops
  discriminating.

  How badly it stops discriminating was the finding. Measured with uprobes and
  uretprobes on a symbolized `claude --help`, exact counts from one run — not
  inferred from the profile:

  | probe | registrations | calls | answered "yes" |
  |---|---|---|---|
  | `is_registered_buffer_slow` | 10 | 4,650,058 | **4** |
  | `lookup_registered_typed_array_kind` | 42 | 3,566,956 | **0** |

  1.16 million probes per "yes" for buffers, and not one "yes" in three and a
  half million for typed arrays — every one of them going out of line to a
  thread-local resolution, a `RefCell` borrow and a hash. The typed-array probe
  additionally consults a direct-mapped negative cache whose cold miss *writes
  back*, dirtying a shared cache line to record an answer nothing asked twice.

  `RegistryAddrWindow` is the same monotone idea applied to the address rather
  than to the fact of registration: a process-global `[lo, hi]` that every
  registration widens *before* it publishes, checked inline at the call site. An
  address outside it cannot be in any table the window covers, so rejecting is
  sound; accepting falls through to the exact lookup that was already there. It
  is strictly stronger than a latch — an unregistered process has the empty
  window `[usize::MAX, 0]`, which contains nothing.

  It removes 98.0% of the buffer probe's calls and 97.2% of the typed-array
  probe's (4,650,058 → 92,965 and 3,566,956 → 100,926, measured the same way).

  It is deliberately **not** a `GcHeader` tag test. A registered typed array is
  not required to have a readable `ptr - GC_HEADER_SIZE` (see the `mprotect`ed
  guard-page fixture in `promise::combinators`), and `native_arena`'s
  `native_memory_copy_rejects_buffer_registry_forged_to_old_non_buffer` pins
  that a registry entry may legitimately disagree with the header type. The
  window never dereferences the candidate, so neither case can be misclassified.

  `buffer::header` already had this filter as the thread-local
  `BUFFER_ADDR_RANGE` — but *behind* the call, where it still paid the call, the
  prologue, two thread-local resolutions and a tail call into `is_shared_sab`
  for every rejection. The window is the same test hoisted in front of the call
  and widened to cover the external and `SharedArrayBuffer` registries too, so
  those routes keep their existing behaviour.

  `is_closure_ptr` consults no registry at all; its cost was ordering. It is a
  conjunction of a handle-band check, a heap-floor check, an alignment check,
  arena ownership plus a GC-header read, and an exact `CLOSURE_MAGIC` tag — and
  the tag ran last. Measured over 2,240,934 calls, the tag alone partitions them
  231,704 / 2,009,230, which is bit for bit the partition the whole function
  produces: in that run it decided every answer, while `classify_heap_generation`
  and the header read ran on 100% of calls to change none of them. The tag now
  runs first. Arena ownership still runs, below, where it does its actual job of
  refusing a coincidental "CLOS" left in recycled arena storage — pinned by
  `managed_error_with_closure_magic_in_padding_is_not_a_closure`, which writes
  the magic into an `ErrorHeader`'s padding and demands `false`.

  Measured on `claude-code` (`cli_2.1.112.js`, `--help`), both arms built from
  `b3f14e9cde` in one session on the same host, `PERRY_DEBUG_SYMBOLS=1`,
  11 interleaved reps:

  | | instructions (min / median) | cycles (min / median) |
  |---|---|---|
  | before | 7,160,271,524 / 7,163,283,415 | 3,446,625,309 / 3,479,536,202 |
  | after | 6,791,077,579 / 6,793,829,953 | 3,229,414,851 / 3,267,521,023 |
  | | **−5.16% / −5.16%** | **−6.30% / −6.09%** |

  Cycles fall by *more* than instructions, and IPC rises (2.077 → 2.102), so
  none of this is work that was riding free in superscalar slack — the check
  that a previous change in this campaign failed, having removed 24% of its
  instructions to move cycles by +1%. The probe family's share of a symbolized
  profile goes 6.86% → 2.65%: `is_registered_buffer_slow` 2.33% → 0.21%,
  `lookup_registered_typed_array_kind` 1.30% → 0.11%, `is_closure_ptr`
  1.23% → 0.42%.

  Output stays byte-identical to `node cli_2.1.112.js --help` (9,175 bytes,
  rc=0) on both arms; every number above is gated on that. Re-measuring the
  answer distribution on the shipped binary confirms the window kept all four
  of the run's genuine "yes" answers while removing 98.03% of the calls.

  The same shape still fits four more probes that this change does not touch,
  now the largest remaining members of the family: `is_registered_symbol_slow`
  (0.60%), `is_registered_class_prototype_object` (0.47%), `is_registered_box_ptr`
  (0.33%) and `is_uint8array_buffer_slow` (0.21%).

### Fixed

- **`RegistryAddrWindow::admit` cannot drop a registration.** It is two
  unconditional `AcqRel` `fetch_min`/`fetch_max` calls, with no "already
  covered?" pre-check, because two earlier drafts of that one function were
  wrong in two different ways and a dropped registration here is a misclassified
  pointer, not a slow path.

  A `load` then `store` is a read-modify-write with a hole in it: two threads
  registering at once both read the old bound and the *narrower* of the two
  stores can land last, evicting the other thread's live registration from the
  window. A `Relaxed` "skip if already covered" pre-check is the same bug moved
  up into the memory model: a thread that skips the RMW because it *observed*
  another thread's widening performs no acquire, so that widening never joins
  its happens-before graph, and a reader synchronising with only this thread's
  subsequent publish is not guaranteed to see the bound that covers the address.
  Registration runs 52 times across a 6.9-billion-instruction `claude --help`,
  so neither fast path bought anything measurable.

  Both probes now re-derive every window rejection from the authoritative tables
  under `debug_assertions`. The window is sound only if *every* route into the
  guarded tables admits first; an enumeration of those routes is a snapshot a
  later commit can invalidate in silence, so the enumeration is machine-checked
  instead — a registration route added without `admit` panics in the first test
  that exercises it. Compiled out entirely in release.

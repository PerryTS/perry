### Performance

- **The symbol and class-prototype probes answer "no" from an inlined Bloom
  test instead of a mutex acquisition and a linear scan; the Uint8Array probe
  gets #9272's address window.**

  #9272 put an inline `[lo, hi]` address window in front of `is_registered_buffer`
  and `lookup_typed_array_kind`, and named four probes as the next-largest
  members of the same family. Measured on a symbolized `claude --help`
  (`is_registered_symbol_slow` 0.65%, `is_registered_class_prototype_object`
  0.46%, `is_registered_box_ptr` 0.22%, `is_uint8array_buffer_slow` 0.22%), a
  window is the right fix for exactly one of them.

  Exact uprobe/uretprobe counts from one run — not inferred from the profile:

  | probe | calls | answered "yes" | a window rejects | a filter rejects |
  |---|---|---|---|---|
  | `is_registered_symbol` | 378,163 | 622 | 38.3% | **99.58%** |
  | `is_uint8array_buffer` | 537,921 | **0** | **100%** | — |
  | `is_registered_class_prototype_object` | 26,290 | 122 | 54.0% | **99.05%** |
  | `is_registered_box_ptr` | 211,148 | **205,640 (97.4%)** | — | — |

  The window column is not a guess: it replays each probe's real argument stream
  against the window its own registrations would have built, widening it exactly
  as `RegistryAddrWindow::admit` does. The filter column does the same against a
  bit-for-bit simulation of the filter that ships here.

  **Why a window fails on two of them.** Buffers and typed arrays sit in their
  own allocations; symbols and class prototypes are ordinary `gc_malloc`'d heap
  objects, interleaved with everything else the program allocates, so `[lo, hi]`
  grows to span ~280 MB of heap and stops discriminating. `RegistryAddrFilter`
  is the same monotone contract — admit before you publish, bits only ever
  set, `false` means "definitively absent" — over a 1,024-bit Bloom filter
  instead of a range. Both shapes are in the tree on purpose; pick by
  measurement.

  For symbols this replaces #9177's `[lo, hi]` range, which sat *inside*
  `is_registered_symbol_slow`: a rejected probe still paid the out-of-line call
  and a `OnceLock` load for an env kill switch before reaching the two bound
  loads. `PERRY_SYMBOL_RANGE_FILTER` is gone with it — it guarded an
  enumeration of the inserters, and the debug-build audit below is strictly
  stronger than an env var nobody sets.

  For class prototypes the filter sits in front of a `map.values().any(…)`
  **linear scan** (#9225) reached through a thread-local and an `RwLock`, whose
  one caller — `descriptor_state::disable_inline_guards_for_descriptor_target`,
  100% of the 26,290 calls — runs on every `Object.defineProperty`, and a bundle's
  `__export(exports, { … })` init runs thousands of those. This does **not**
  close #9225: a false positive still pays the scan, so the table's O(n) slope
  survives at ~1% of its strength, and the O(1) inverse index that issue asks
  for is still the right structural fix.

- **`is_registered_box_ptr` is not a member of this family, and the measurement
  is what says so.** It answers **"yes" 205,640 times out of 211,148** (97.4%)
  — the opposite regime from every other probe here, because its four callers
  (`js_closure_set_box_capture_ptr`, `js_box_get_bits`, `js_box_set_bits`,
  `js_box_capture_cell_ptr`, 100% of calls between them) ask it about *actual
  box pointers* on the async-locals read/write path. A filter in front of it
  could remove at most 2.6% of its 0.22%. It is left alone deliberately; if it
  is attacked again, the target is the cost of the **hit** — the direct-mapped
  positive cache in `tls_hot` — not a rejection filter.

#### Why it cannot misclassify

These probes classify pointers, so a wrong answer is type confusion rather than
a slow path. A Bloom filter has false positives and no false negatives, which
is the asymmetry the probes need: a false positive costs the ordinary lookup
that was already there.

That leaves one obligation — every registration must admit before it publishes
— and it is not left as an enumeration of writers. Enumerations of writer sets
have produced silent wrong answers in this codebase twice recently, so under
`debug_assertions` **every rejection is re-derived from the authoritative
table**: `SYMBOL_POINTERS` for the symbol filter, `CLASS_PROTOTYPE_OBJECTS` for
the prototype filter, `is_uint8array_buffer_slow` for the Uint8Array window. A
registration route added without admitting panics in the first test that
touches it. The class-prototype table is the harder case — the two GC root
scanners and the per-slot GC step all *rewrite* stored addresses through
`visit_usize_slot`, so each admits what the visitor left behind, under the same
write guard that keeps the new address unfindable until it drops.

`admit` performs its bit-set RMWs unconditionally, with no "already set?"
pre-check, for the reason #9272 gives for the window's: a thread that skips the
RMW performs no acquire, so another thread's admission never joins its
happens-before graph and a reader synchronising only with this thread could
miss it.

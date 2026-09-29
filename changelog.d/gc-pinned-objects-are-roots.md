**GC: a pinned object is a root, marked and traced like any other object.**
Every mark entry used to treat `GC_FLAG_PINNED` as "already marked", so a
pinned object was kept but never traced, and a child reachable only through it
was freed. `js_promise_new_cross_thread` (bcrypt, sharp, container compose,
worker_threads, thread spawn) lost its `then`/`await` reaction closure after
one full collection: a use-after-free when the native side resolved.

- A pin now means only "don't move, don't sweep". The mark entries in
  `gc/trace.rs` and `gc/roots.rs` no longer short-circuit on it.
- Pinned objects are found as roots through the header bit plus a per-block
  `pinned_summary` (arena) and a malloc-registry summary, both set only by the
  pin setters in `gc/pin.rs`. Leaf objects need no root.
- The full trace's block persistence no longer counts a pinned header as live.

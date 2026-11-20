### Documentation

- **Retracts an unsound optimisation the plan was recommending (#7588).** Docs
  only, but the retracted text pointed the next reader at a use-after-free.

  #7581 proposed emitting the typed-shape install's hit path inline at the `new`
  site, by analogy with #7566 — every argument but the object pointer is a
  compile-time constant for the class, so it looked like the same win. #7586
  tested both halves of that reasoning and **both are wrong**.

  **The frame is not the lever; inlining it is a regression.** The prologue does
  match #7566's shape — `sub sp, sp, #0x150`, six `stp` pairs, a 336-byte frame
  and twelve callee-saved spills per construction, sized by LLVM for a descriptor
  build that runs once per shape. Outlining it behind `#[cold] #[inline(never)]`
  cut the frame to 80 bytes and the spills to zero and made it **slower**:
  `push_cls` 0.72 → 0.75 s, `churn_alloc` 0.72 → 0.79 s. Those spills are cheap
  dual-issued stores off the critical path, and keeping six arguments live to
  forward to the outlined call costs more in register moves than the prologue
  saves. The function is bound by **instruction count, not frame size** — which
  is why #7586's shipped fix attacks instruction count and gets 1.091×.

  **⛔ The codegen half is a use-after-free factory**, and it is dangerous
  precisely because it is free: `declare`-path classes must have an empty pointer
  mask, so since #7566 the inline `new` already writes its `GcHeader` as one i64
  constant, and OR-ing `GC_OBJ_TYPED_LAYOUT_INTACT` into it costs +0 instructions
  and +0 bytes.

  It breaks the unwritten invariant **"intact ⟹ a descriptor is reachable"**,
  which `layout_note_slot` silently depends on. On a contradicting store to an
  intact-but-descriptor-less object the probe resolves `None`, and
  `layout_set_typed_unknown` — the only thing that clears the intact bit — is
  reachable **only from the `Some(verdict)` arm** (`gc/layout.rs:782–788`). So
  control falls through to the pointer-mask path and the bit is never cleared.
  The object is thereafter `SIDE_MASK` to the collector and *intact* to the
  class-field inline guard, which consults no map by design; the raw-store fast
  path then writes a double over a pointer slot with no barrier and no layout
  note, and the next collection walks it as a heap pointer.

  Flagged for whoever reads that code next: the comment at `gc/layout.rs:742`
  says a `None` verdict "can only cost an extra fall-through, never mis-track a
  slot". That is true **only while the invariant holds** — it is a consequence of
  the invariant, not an independent guarantee, and it reads as reassurance to
  exactly the person about to break it.

  Both are now recorded in `docs/engine-plan.md` as ⛔ entries carrying their
  measurements, so the next person re-deriving the same obvious idea meets the
  result rather than repeating the experiment.

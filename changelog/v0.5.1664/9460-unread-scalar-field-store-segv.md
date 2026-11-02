### Fixed

- **SIGSEGV storing to a scalar-replaced object-literal field that is never
  read.**

  ```js
  "use strict";
  const o = { x: 1 };
  o.x = 7;                 // SIGSEGV — nothing reads o.x
  ```

  ```js
  const o = { x: 1 };
  for (o.x of [7]) {}      // SIGSEGV, sloppy or strict
  const p = { x: 1 };
  p.y++;                   // TypeError "Cannot assign to read only property 'y'"
  ```

  Three lines of ordinary code, in both modes. The fault is `str d0, [x8]` with
  `x8 = 0x10` — a raw field store through a **null** receiver at
  `null + sizeof(ObjectHeader)`.

  `stmt/let_stmt.rs`'s scalar-replacement arm elides the heap allocation for a
  non-escaping `new` and gives each field a stack alloca. For the synthetic
  `__AnonShape_*` class an object literal lowers to, it creates slots only for
  the fields in `non_escaping_new_used_fields` — which tracked **reads** only,
  on the argument that a store nothing ever reads is unobservable and its slot
  can be elided. That is true of the *store* and false of the *slot*: the same
  arm registers `ctx.locals[id]` as an **uninitialized dummy alloca** (the
  binding has stopped being an object), so a store lowering that looks up the
  field slot and finds none does not stop — it falls through to the class-field
  / `Ptr<Shape>` lanes, which load that dummy as an `ObjectHeader*`.

  The read side has had the matching guard since the synthetic-shape work
  (`expr/property_get.rs`, whose comment names this exact hazard: "the generic
  runtime helper that crashes on the dummy slot"). The write side never got it —
  and it needed it on **three different lanes**: `Expr::PropertySet` (`o.x += 1`,
  `for (o.x of …)`), `Expr::PutValueSet` (`o.x = v`, via
  `try_lower_sloppy_class_field_store` and the write IC), and
  `Expr::PropertyUpdate` (`o.y++`). So the fix is at the source, in the two
  collectors that decide which fields get slots, rather than in each lane:

  - `collectors/escape_news.rs` — `non_escaping_new_used_fields` now counts a
    WRITE as a use, so a written field always has a slot. This is #9024's rule
    one step further: #9024 escapes a write to an *undeclared* property because
    it would have no slot; this gives a slot to a *declared* property that would
    otherwise have none. It costs nothing at runtime — a store into an alloca
    nothing loads is removed by LLVM. The walker also had **no arm at all** for
    `Expr::PutValueSet`, which is what `o.x = v` lowers to, so neither the
    written field nor the value's own nested uses were being recorded.
  - `collectors/escape_check.rs` — the `Expr::PropertyUpdate` arm gains #9024's
    `class_chain_has_field` check, which the `PropertySet` and `PutValueSet`
    arms already had. `o.y++` on an undeclared property has no slot either.
  - `expr/property_set.rs` — a backstop mirroring `property_get.rs`: a store to
    a scalar-replaced local with no field slot lowers the value for its side
    effects and discards the store (`ScalarObjectFieldSetElided`), the same
    shape the `this` arm below it has always had for an inlined constructor
    whose target field has no slot. With the collector fixes above this should
    no longer be reachable; it is kept because the failure mode it prevents is a
    null-pointer store, and because the read side carries the identical guard.

  Two things worth recording about the report, which said the crash "does not
  reproduce in isolation — the preceding throws are required":

  - It reproduces in **three lines with no exception at all**. The original
    isolated attempt printed `o.x` afterwards, and that read is what creates the
    slot and hides the crash. "Several rejections first" was the shape it was
    found in, not the condition.
  - It is **not** specific to sloppy mode, so it survives the #9459 fix rather
    than being masked by it. Neither the `perry_sjlj_try` transport (#9323) nor
    a rooting hole (#9417/#9444/#9445) is involved: `PERRY_GC_PROTECT_FROMSPACE`
    changes nothing, because the address was never a heap object.

  - `test-files/test_gap_9460_unread_scalar_field_store.cts` — every write
    spelling (`=`, `+=`, `o[k] =`, `o.x++`, `o.y++`, `for (o.x of …)`,
    `[o.x] = arr`, a brand-new field) against an unread scalar-replaced field in
    both modes, each printing a sentinel INSTEAD of reading the field, because
    reading it is what hides the bug. Controls: the RHS side effects must still
    happen, the read-after-store versions must still read back what was stored,
    and an escaping receiver must keep its real heap object.

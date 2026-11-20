### Documentation

- **The engine plan's construction profile is now the v0.5.1325 one, not the
  v0.5.1299 one (#7581).** Docs only.

  Three rows of the `#7469` decomposition are closed, and the plan still
  presented them as the backlog. Re-measured in #7578 (two independent `sample`
  runs of `push_cls`, leaf shares): **`_tlv_get_addr` is 1.0%**, from 27.0%
  (closed by #7565), and **`layout_forget_object` is 1.7–2.9%**, from 14.5%
  (closed by #7525/#7532). Neither is a lever any more. Nothing regressed — the
  rows that remain grew as *shares* because everything around them collapsed.

  The new top lever is **`gc::layout::typed_shape_layout_entry` at ~25%**, tied
  with write barriers (~25%, #7511). It is characterised rather than merely
  named: it is **not** the `ValidateSlots` loop, because `push_cls` takes the
  `js_gc_declare_typed_shape_layout` path (confirmed in the emitted IR — one
  call to `declare`, zero to `init`), so #7515/#7532 are working as intended. It
  is the install itself, whose hit path `layout.rs:1022` documents as reducing to
  "the two header bit-writes `shape_install_shared` would have performed",
  reached through an FFI call whose every argument but the object pointer is a
  compile-time constant for the class. That is the same shape #7566 just won
  1.81× on.

  Records that **#7512 is closed by #7515** — not by diffuse side effects, which
  is how a first pass at this attributed it. The root cause generalises and is
  the reason it is written down rather than just closed: the dead-field-init
  elision matched `Expr::PropertySet`, which the compiler *synthesizes* for
  anon-shape literal constructors, while every source-level `this.v = v` lowers
  to `Expr::PutValueSet`. **Nothing a user can type produces `PropertySet`**, so
  the elision was structurally unreachable for the declared class it was
  documented as covering, and every class construction paid two extra IC diamonds
  writing an `undefined` that the next statements overwrote. *An unreachable
  predicate passes every soundness test there is* — #7486 was correct in
  everything it asserted and did nothing on the case it named. A predicate over a
  synthesized-vs-source IR distinction needs a test that the **source form**
  reaches it, not only that the transform is sound when it fires.

  Finally it flags the one row in #7578 that is **unexplained**, so it is not
  worked from as written: `js_array_length` at 10–15%, against a single call site
  executing 20,000 times in a 20,000,000-push workload. Both isolation attempts
  failed and are recorded on the issue — varying array size moved the workload
  into a different GC regime (leaf samples 379 → 5,816, the top rows becoming
  btree and remembered-set work), and a `.length`-only microbenchmark measured
  1.00 ns/iter for **both** a 1,000- and a 10-element array, which looks like a
  clean O(1) proof and is not one: that is an empty loop, because the read is
  loop-invariant and was hoisted. A microbenchmark that proves a runtime call is
  free has usually proved that it was deleted.

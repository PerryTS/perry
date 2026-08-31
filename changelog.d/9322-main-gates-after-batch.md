### Fixed — three `lint` gates that went red on `main`

- **`shape_descriptor_census.py` matches the #9317 stamp funnel.** The census
  asserts the shape descriptor is published before the `ObjectHeader` ShapeId,
  and found that write by its literal spelling. #9317 correctly routed every
  post-birth publication through
  `stamp_object_shape_id_with_carrier_note`, so the inline write is gone and
  the assertion could no longer locate it. The ordering itself is unchanged —
  `shape_descriptor_ensure_with_holes` still precedes the stamp in
  `publish_object_shape_from`. The census and its own inversion self-test now
  name the funnel.

- **An unused `Path` import** in `strided_tagged_fill.rs` (#9316), which
  `cargo check --workspace --all-targets -D warnings` treats as an error. It
  is invisible to a plain `cargo build`, which does not compile test targets.

- **`unrooted_local_shape.py` per-file ceiling** for `perry-ext-mysql2`
  (#9319). Its two new tests allocated eight heap values up front and then
  pushed them one at a time, leaving every earlier value live across a
  `js_array_push` that can move it — the #8217 shape. Each value is now built
  inside the iteration that pushes it, and the array is consumed in the
  expression that builds it. The file returns to its ceiling of 9 with no
  baseline change, and the repository total drops 576 → 567.

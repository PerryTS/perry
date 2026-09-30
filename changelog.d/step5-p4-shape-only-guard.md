- codegen: the class-field read, write and loop-preheader guards and the
  element-shape residual no longer test the per-object typed-layout bit; the
  ShapeId compare alone licenses a raw-f64 slot (charter step 5, P4).

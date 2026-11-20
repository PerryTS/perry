### Changed

- Multi-arm direct method call sites (the subclass-arm compare chain, the indexed-dispatch shape probe and the short-spread method probe) resolve the receiver's `(class_id, ShapeId)` inline instead of through the runtime probe `js_method_direct_shape_class`: the same prototype-latch, pointer/heap-band, header-word and non-zero tests, ending in two `select`s so the zero-on-decline convention the chains rely on is unchanged. On the wolf-ecs cycle the probe was 2.2–2.6% of self time.

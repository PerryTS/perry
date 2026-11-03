### Fixed

- A 1-argument `.forEach(cb)` on a `Map` or `Set` no longer iterates nothing
  when codegen could not statically prove the receiver was a collection
  (`obj.someSet.forEach(cb)`, react-server-dom's `request.abortableTasks`).
  Codegen fuses that shape to the array entry point `js_array_forEach`, whose
  #5989 collection reroute sat behind `normalize_array_receiver`; #8041 widened
  `clean_arr_ptr` to reject every tracked non-array, which nulls a
  `GC_TYPE_SET`/`GC_TYPE_MAP` receiver and left the reroute unreachable. The
  reroute now runs first, receiver-tag gated so an ordinary array still never
  reaches a registry probe (#8117).

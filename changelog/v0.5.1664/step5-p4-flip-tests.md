- runtime: the old layout-state collector tests now assert the shape protocol: a
  lying `POINTER_FREE` layout state no longer strands a child (the shape traces
  every `Any` lane), and raw-numeric skipping is the shape's `F64` lanes, which
  the layout-scan telemetry now counts (charter step 5, P4 flip).
- runtime: `Object.assign` hands its rooted target and keys to the write funnel
  through `with_mut_ptr`/`with_const_ptr`; its raw-handle debt drops 28 -> 7 and
  the ledger records the relocation from `object/alloc.rs`.

### Fixed

- Refresh the Windows classifications of `js_thread_global_materialize` and
  `js_thread_global_publish` from the generated table produced by the documented
  Windows archive classifier at the reviewed CI repair head. Materialization is
  `AllocOnly` and publication is `Leaf` on this target. Preserve every other
  table entry and all classifier rules and seeds, removing two conservative
  drifts that fail the strict full-tier check.

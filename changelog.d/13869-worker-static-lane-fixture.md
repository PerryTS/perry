Tests: the worker static-lane test in `constfn_method_lanes.rs` reassigned
`o.m` to an arrow literal. Since FVA, the facts prove that `m` holds exactly
those two bodies, and the proven field call lowers the site before the method
site, so no static lane was emitted and the test failed. The test now stores a
value the facts cannot name a body for (`globalThis.noSuch ?? arrow`), so it
covers the static lane in a worker and after the reassignment again. The
compiler is unchanged.

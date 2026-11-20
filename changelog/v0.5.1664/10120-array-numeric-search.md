Fixed `Array.prototype.indexOf`/`includes` on a proven-numeric dense array
(no holes, no NaN-boxed pointers) costing up to ~20x Node: every element was
routed through `js_jsvalue_equals`/`js_jsvalue_same_value_zero`, both
`#[no_mangle] extern "C"` call boundaries the optimizer can't inline. A
specialized scan now collapses the search to a bounded `f64` compare loop
when that proof holds, falling back to the existing generic walk otherwise
(exotic iteration, mixed-kind arrays). Measured ~9-16x faster on this host
across n=1k..1M, with identical results (#10092).

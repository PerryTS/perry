`timer_object` (`crates/perry-runtime/src/timer.rs`) — new in this branch —
read the rooted handle object's address back out with a bare
`get_raw_mut_ptr` three times, pushing the module from its recorded ceiling of
7 raw-handle debt sites to 10.

The three are the same scoped-argument shape as their `text.rs` twin (both
allocate an ordinary object, link its prototype, then stamp a packed state word
into its meta), so they become `with_mut_ptr::<ObjectHeader, _>(…)`. The
`unsafe` block around `object_meta_ensure` moves inside the closure rather than
wrapping it, which also drops a level of nesting. timer.rs returns to exactly
its existing ceiling of 7 — no ceiling was raised — and the recorded total
returns to its 906 baseline.

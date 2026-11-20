Fixed a 22x element-access cliff on literal-length typed arrays (#11810). An
alias of a proven typed-array view, including the temp that `a[k] -= v`
spills its receiver into, demoted both names to the runtime
`js_typed_array_get` path. A module-level `new Float64Array(35)` driving an
n-body kernel ran 13.33G instructions against 5.62G for a computed length.
The alias now shares the source's view (same data pointer slot, same alias
scope), and the kernel runs 0.59G. Also fixed a miscompile: reassigning an
alias of a `Uint8Array` to a new buffer redirected the source's later writes
into the new buffer.

A `var` re-declaration of a parameter now demotes the function's specialized
numeric entry, as an assignment to the parameter always did. The re-declaration
reuses the parameter's id as a `let`, which the demotion scan did not count, so
`var n = buffer; new Int32Array(n)` kept `n` proven numeric and built an owned
array of that length instead of a view over the buffer (#11802).

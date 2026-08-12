### Align incremental-barrier gate ordering across generated and runtime code

Generated write and root barriers read the incremental-mark active counter
with LLVM's sequentially-consistent ordering even though runtime barriers made
the same decision with Rust's relaxed ordering. The generated gates now use
LLVM `monotonic`, and the remaining shadow-stack runtime gate shares the
runtime's relaxed helper.

The counter publishes no accompanying memory: arming increments it before
installing the current thread's barrier pointer, while disarming clears that
pointer before decrementing it. It is therefore authoritative only for whether
the current thread needs the TLS-backed barrier call, and needs no acquire
relationship with the thread-local pointer. Tests pin the relaxed ordering for
all three generated gate families and exercise the live armed-barrier premise.

On `interp`, the change replaces 735 `ldar` instructions with matching `ldr`
instructions while preserving exact output and executable size. Two
order-reversed 31-pair sweeps on the quiet M1 mini found no measurable runtime
change (0.6288 s to 0.6284 s median; best cycles -0.14%).

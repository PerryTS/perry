Serve function-valued stores from the emitted store cache. Storing a closure
into an object (`this.constructor = F`, `this.cb = fn`, `o.m = function(){}`)
gives its slot a ConstFn lane naming the body, and both the key-add memo and
the existing-key word refused such a lane, so every store took the runtime
miss (about 2,800 instructions per key-add). A site now records one ConstFn
body (a fifth site word); a ConstFn-flagged entry hits only for a closure of
that body (a GC-header kind test, an info compare and a capture-flag test),
and any other value keeps the checked miss, which deprecates the lane as
before. Micro rows: `new C()` storing `this.k = C` 5,013 -> 2,189
instructions/op, an `o.cb = fn` overwrite 2,026 -> 202; the decimal.js-shaped
#10507 row 5,003 -> 2,387.

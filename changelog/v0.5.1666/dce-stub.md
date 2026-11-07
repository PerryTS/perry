Compiler switch `PERRY_FACTS_DCE=1` (off by default, part of the object-cache key
through the module digest) replaces every body the whole-program reach calls
inactive with an `llvm.trap` entry. The one predicate is `BodyReach::inactive`
(solve complete, body classified by the activation pass, no path reaches it);
an incomplete reach, a missing or a new identity is active. With
`PERRY_VERIFY_FACTS=1` too, the claim report runs first. Off, emitted LLVM and
the linked code are byte-identical. The switch stays off until the false inactive
claims the check mode found are fixed.

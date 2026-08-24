Versioned eligible instance methods for an exact `undefined` optional argument.
The public boxed-ABI wrapper validates the live argument bits once and sends only
that value to a private specialized body; functions and every other falsey or
non-callable value retain the ordinary JavaScript path. Mutation, closure capture,
async/generator bodies, and oversized methods are conservatively excluded.

This removes the per-entity optional-filter truthiness and callback arm from
codehz/ecs's 10k-entity accumulation loop. On an Apple M1 Mac mini, 11
alternating process pairs measured 0.179851 ms versus 0.195838 ms on the exact
parent, an 8.126% median paired improvement with 11/11 wins and all output
oracles passing.

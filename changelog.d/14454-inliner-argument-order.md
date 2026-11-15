The function and method inliner put a call's argument where the inlined body
reads the parameter, so the argument was read after whatever the body did
first. `g(m)` with `g(a) { return bump() * 0 + a }` and `bump` writing `m`
returned the new value of `m` instead of the value at the call; the same
happened for statement bodies, void methods, a caller's parameter or local
written by a closure the body calls, and a global argument. An argument is now
substituted only when it is a literal or a local that nothing in the module
writes (the rule the closure-local rewrite uses); every other argument is
bound before the body, in argument order. A method parameter that the method
body writes now gets its own copy too, as a function parameter already did.

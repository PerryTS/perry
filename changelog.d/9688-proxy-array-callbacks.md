Array iteration methods and sorting now accept callable `Proxy` callbacks,
matching Node for proxy-wrapped functions and bound functions.

Comparator validation no longer treats a Proxy registry handle as a closure
pointer, preventing a user-triggerable crash. Non-callable callbacks and
comparators continue to throw Node-compatible `TypeError`s.

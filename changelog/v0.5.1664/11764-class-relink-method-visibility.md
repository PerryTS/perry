After `Object.setPrototypeOf(C.prototype, X)`, methods, getters and setters
declared in C's old parent class are no longer visible on C's instances.
Property reads, `in`, calls and `super.m()` follow the live prototype chain, so
the new chain's members resolve. A wide dispatch tower also stops running a
parent method's old body after `Object.defineProperty` replaces it on the
parent prototype.

Callable proxies returned by a getter on the replacement super chain use the rooted proxy-call helper. Object-target proxies remain non-callable, and nested/revoked proxy calls and getter exceptions retain their normal behavior.

Include the immutable thread-global IR suite in scoped test selection after
integrating current main, so the complete suite map accepts this branch.

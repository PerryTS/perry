An arrow in a method of a derived class no longer throws "Must call super
constructor" when the method runs while a derived constructor is inside
`super()` (a base constructor calling an overridden method). Only arrows
written in the derived constructor itself, directly or through enclosing
arrows, check the constructor's pending `this` binding. The Next.js
standalone server's `NextNodeServer` hit this through
`getinterceptionRoutePatterns -> getRoutesManifest -> tracer.trace(() => this...)`.

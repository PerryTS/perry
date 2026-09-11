Fix dynamic calls to a closure's own `bind`, `call`, `apply`, and `toString`
properties. These overrides now take precedence over Function.prototype's
intrinsic dispatch, including native constructors such as AsyncResource and
AsyncLocalStorage with their own static `bind` methods. Own accessors are
invoked once, and non-callable own properties throw instead of falling through
to an intrinsic. Ordinary Function.prototype fast paths remain unchanged,
including `apply` with an arguments object.

Adds a runtime regression and a bounded, application-independent native matrix
covering builtin/import/require/alias forms, async context and receiver capture,
custom own methods/accessors, non-callable overrides, and intrinsic fallbacks.

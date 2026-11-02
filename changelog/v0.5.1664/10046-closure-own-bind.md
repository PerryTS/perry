Fix dynamic calls to a closure's own `bind`, `call`, `apply`, and `toString`
properties. These overrides now take precedence over Function.prototype's
intrinsic dispatch, including native constructors such as AsyncResource and
AsyncLocalStorage with their own static `bind` methods. Own accessors are
invoked once, and non-callable own properties throw instead of falling through
to an intrinsic. Ordinary Function.prototype fast paths remain unchanged,
including `apply` with an arguments object.

Named builtin ESM imports now install their module's dispatch/attachment bucket
before initializing the export snapshot. This prevents an early named import
from caching an AsyncResource/AsyncLocalStorage constructor without its own
static methods or prototype. Installation remains per-module, so unrelated
native modules remain eligible for dead stripping.

Adds a runtime regression and a bounded, application-independent native matrix
covering builtin/import/require/alias forms, async context and receiver capture,
custom own methods/accessors, non-callable overrides, and intrinsic fallbacks.
The standalone CI suite prepares coherent native providers outside its fixture
timeouts, uses the pinned Node oracle, and publishes failed compiler output.

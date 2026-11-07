Implement `Worker.prototype[Symbol.asyncDispose]()` using the existing termination
lifecycle, with async function metadata, dynamic `terminate()` overrides, and
Promise rejection and exit-ordering parity.

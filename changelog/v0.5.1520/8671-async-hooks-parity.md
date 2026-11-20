### Fixed

- Complete the `node:async_hooks` parity tracker across all 194 fixtures:
  hook mutation and lifecycle ordering, Promise/resource identity and trigger
  chains, `AsyncResource` and `EventEmitterAsyncResource` subclasses,
  `AsyncLocalStorage` propagation, and provider lifecycles for timers, files,
  DNS, crypto, zlib, processes, signals, workers, streams, net, HTTP(S), TLS,
  readline, event iterators, ESM, fetch, and UDP now match Node.

  Native crypto completion callbacks (`pbkdf2`, `scrypt`, `hkdf`, `argon2`,
  `generateKey`/`generateKeyPair`, `generatePrime`, `checkPrime`, `randomInt`,
  `randomFill`) are now delivered on the check phase inside their own provider
  resource instead of being invoked inline, matching Node's threadpool
  completions and joining the async `randomBytes` form. The scheduled
  completion is a ref'd event-loop handle, so `js_callback_timer_has_pending`
  keeps the loop alive until it fires.

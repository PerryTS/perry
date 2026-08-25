### Fixed

- Complete the `node:async_hooks` parity tracker across all 194 fixtures:
  hook mutation and lifecycle ordering, Promise/resource identity and trigger
  chains, `AsyncResource` and `EventEmitterAsyncResource` subclasses,
  `AsyncLocalStorage` propagation, and provider lifecycles for timers, files,
  DNS, crypto, zlib, processes, signals, workers, streams, net, HTTP(S), TLS,
  readline, event iterators, ESM, fetch, and UDP now match Node.

### Fixed

- Node-compatible MessageEvent delivery now defaults undefined data to null for BroadcastChannel and MessagePort DOM listeners, including parentPort, without changing raw listeners, synchronous receive or Perry-only web delivery.

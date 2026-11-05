### Fixed

- BroadcastChannel now shares EventTarget listener and handler dispatch, including borrowed receivers, persistent handler order, capture/once/AbortSignal behavior, object listeners, and manual dispatch after close, without a parallel callback registry.

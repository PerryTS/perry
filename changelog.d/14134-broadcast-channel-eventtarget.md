### Fixed

- BroadcastChannel now shares EventTarget listener and handler dispatch, including borrowed receivers, persistent handler order, capture/once/AbortSignal behavior, object listeners, and manual dispatch after close, without a parallel callback registry.
- Shared EventTarget dispatch tracks removal on existing listener records, supports callable proxies, preserves once semantics after constructor mutation, and reports callback exceptions asynchronously without skipping listeners. Handler values and receivers remain rooted across moving collection; closing releases the provider transport without clearing object-owned listeners.

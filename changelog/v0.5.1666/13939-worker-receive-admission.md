### Fixed

- `worker_threads.receiveMessageOnPort()` now rejects invalid receivers and closed BroadcastChannels without reading spoofable public IDs, while accepting closed MessagePorts and synchronously consuming runtime-backed port inboxes.

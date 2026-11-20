### Fixed
- Attach native `WebSocketServer({ server })` instances to an existing HTTP listener; deliver manual `handleUpgrade` callbacks and connection events with usable client handles and the original request.
- Bind `WebSocketServer({ port: 0 })` to an ephemeral port and expose the actual listening address through `address()`.
- Treat native handle IDs as identities when hashing Sets, including `WebSocketServer.clients`.

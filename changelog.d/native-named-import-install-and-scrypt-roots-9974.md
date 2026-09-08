### Fixed

- Named imports from `node:stream` now install their callable decorators before
  the export is cached, restoring computed `Readable.toWeb` access and
  `util.promisify.custom` on `stream.pipeline` and `stream.finished`.
- `promisify(crypto.scrypt)` now installs crypto's indirect dispatch and keeps
  its options and completion callback rooted across allocations, preventing a
  silently pending promise after a moving collection.

### Added

- Add a native `bun:sqlite` compatibility facade backed by the same rusqlite
  engine as Perry's `node:sqlite` implementation. `Database` construction,
  prepared statements, positional and named parameters, object and array row
  modes, blobs, safe integers, transactions, change metadata, serialization,
  extension loading, and handle lifetime operations now support OpenCode's Bun
  SQLite adapter without leaving an unresolved `bun:` import in the graph.

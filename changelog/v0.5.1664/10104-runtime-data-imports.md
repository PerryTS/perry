Fix dynamic imports of runtime data-file paths with `with: { type }` attributes
(#10104). Absolute paths and `file://` URLs support TOML, JSON, text, and file
loaders and return a namespace with a `default` export. Invalid TOML or JSON
rejects with `SyntaxError`; runtime code modules keep their deferred error.

Preserve import options through HIR traversal, closure/async transforms,
codegen, and cache hashing. Optimized runtimes retain the TOML parser even
without a Bun import. Regression coverage includes OpenCode's legacy TOML
configuration migration, filename URL decoding, and option evaluation order.

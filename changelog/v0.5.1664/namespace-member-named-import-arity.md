A namespace import's member no longer overrides the arity of a same-named
named import in the importer's flat call maps. effect's `Schema.ts` imports
`{ format }` (2 parameters) next to `* as Duration_` (`format` takes 1), so
`format(source)` lost its `undefined` padding and the callee read a stale
register; OpenCode v2.1.2 segfaulted at startup in `Formatter.format`.

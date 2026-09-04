**Native stream subclasses keep their derived methods when constructor imports
are aliased or minified.** Class heritage now resolves native import bindings to
their original exports before selecting the subclass initializer. This keeps
readdirp's `_read` implementation and the complete EventEmitter method surface
intact, allowing Claude Code's chokidar watcher to initialize without the
`once is not a function` startup errors reported in #9680.

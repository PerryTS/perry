Runtime `import()` failures now name the requested module and explain the native
build's runtime-JavaScript limitation, with guidance to use the application's
Bun/Node distribution or compile the module through a statically resolvable
import. Deferred imports keep their source location and all failures retain
`ERR_MODULE_NOT_FOUND` for optional-dependency handlers. Supported builtin and
compiled imports continue to resolve.

Documents option A for the initial native OpenCode deliverable (#10105, #10107)
and adds runtime diagnostics tests plus a minimized plugin-loader regression
covering one report and continued startup after a configured plugin fails.

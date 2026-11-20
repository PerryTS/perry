Add repeatable `perry compile --define NAME=EXPR` and `perry.json` defines for
build-time constants, including dotted names, JSON snapshots, scoped identifier
replacement, and constant `typeof` guards. Effective defines invalidate build
and object caches while existing `package.json` literal defines remain compatible.

Support `import.meta.resolve(specifier[, parent])` for compile-time module and
asset URLs and runtime package resolution from a directory or file URL. Add an
OpenCode release-define harness and regressions for native worker entry discovery.
Recognize Windows absolute worker paths during module resolution.

**Complete Node.js 26.5.0 `node:module` parity (#6769):** Perry now matches the
full tested Module/CommonJS surface, including resolution and cache lifecycle,
loader hooks, SourceMap behavior, TypeScript stripping, descriptors, and
builtin default/named export identity and synchronization.

The CommonJS wrapper now publishes a real Node `Module` record (`id`,
`filename`, `path`, `paths`, `parent`, `children`, `loaded`, `require`) into the
shared `require.cache`, and the path-module registry's FINAL publication is that
record rather than bare exports — generated `require` sites unwrap `.exports`
through one helper (`path_module_exports`), so the cycle-visible PARTIAL
publication keeps storing the original exports object and Node's circular-require
semantics are unchanged. A statically resolved import that would otherwise be
left `Interpreted` is promoted to native compilation for that FILE only, so
`perry.compilePackages` still bounds runtime-computed package loads.

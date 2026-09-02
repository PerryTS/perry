**`require('child_process').spawn(...)` now actually spawns.** A method call on
the CJS-default namespace object that `require()` /
`process.getBuiltinModule()` returns dispatched under the module name
`child_process.default`, which the runtime's method-call router did not
normalize — so the call found no dispatch bucket and returned `undefined`
without launching a process, throwing nothing.

The tell was the asymmetry. `const f = cp.spawn; f(...)`, `cp.spawn.call(...)`
and `(0, cp.spawn)(...)` all worked, because the property-READ path resolves
through `cjs_default_base_module`. Only the fused member-call form
(`cp.spawn(...)`, `cp['spawn'](...)`) was broken, because the router carried a
second, hand-maintained copy of that table — and it had drifted.
`child_process.default`, `constants.default`, `dns.default`,
`dns/promises.default`, `inspector.default`, `inspector/promises.default`,
`module.default`, `repl.default`, `sea.default` and `wasi.default` were all
missing from it, while `os.default`, `path.default`, `util.default` and friends
were present — which is why the failure looked module-specific rather than
structural.

`const cp = require('child_process'); cp.spawn(cmd, args, options)` is
verbatim what `cross-spawn` does, so every consumer of it inherited the bug:
`execa`, `which` (via `isexe`), and the MCP SDK's `StdioClientTransport`. Under
Perry, claude-code could *be* an MCP stdio server but never *connect* as one —
`claude mcp list` reported `✗ Failed to connect` for every configured stdio
server while `strace` showed no `execve` at all, because
`StdioClientTransport.start()` immediately dereferenced the `undefined` that
came back from `spawn()`.

The router now falls through to the one canonical table instead of duplicating
it, and the normalization is split into `normalize_dispatch_module_name` so the
agreement between the two paths is unit-testable: every `<mod>.default` name in
`cjs_default_base_module` must normalize to the same base module the
property-read path picks, and must reach the same dispatch bucket. Adding a row
to the canonical table without teaching the router about it is now a test
failure rather than a silent `undefined`.

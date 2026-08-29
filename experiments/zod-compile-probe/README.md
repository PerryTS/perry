# Zod `z.compile()` AOT probe

This probe exercises Perry's prototype schema-aware lowering for Zod. The
compiler recognizes a closed `z.object({...})` schema, records a compiler-only
schema IR, and emits an ordinary Perry HIR parser. Zod's generated JavaScript
is not evaluated.

Perry passes that parser to Zod's `compileFromParser()` integration API. Zod's
own source remains responsible for cloning the schema, bypassing the native
path for async/backward/skip-check contexts, installing public parse methods,
and delegating failures to the original parser. Successful parses are
independent of `new Function`/`dyn-eval` without duplicating Zod's wrapper
contract in Perry.

The current proof-of-concept subset is intentionally small:

- `import * as z from "zod"` or `"zod/v4"`
- `z.object()` with static, non-computed fields
- `z.string()`, `z.number()`, and `z.boolean()`
- `z.number().int()`, `.min(<number literal>)`, and `.max(<number literal>)`
- `z.compile(schema)` and the static `{ strict: <boolean> }` option
- immutable `const` schema bindings with unshadowed Zod namespace imports

Unsupported schemas keep the regular Zod call; they are never partially
specialized. Zod versions without `compileFromParser()` also keep the regular
`z.compile()` behavior.

The dependency is temporarily pinned to the source-only package commit that
backs the upstream Zod API proposal in
[colinhacks/zod#6498](https://github.com/colinhacks/zod/issues/6498). Replace it
with the first official Zod release containing `compileFromParser()` once that
API lands. The probe explicitly opts `zod` into `perry.compilePackages`; Perry
therefore resolves `src/index.ts` even though this temporary package has no
built `index.js`, and compiles the complete reachable Zod graph natively.

From this directory, install the pinned Zod dependency, then build and run on
Windows with:

```powershell
npm ci
$env:LLVM_SYS_221_PREFIX = "C:\llvm"
$env:PERRY_RS4GC = "0"
..\..\target\perry-dev\perry.exe compile .\index.ts -o .\zod-aot-poc.exe
.\zod-aot-poc.exe
```

`PERRY_RS4GC=0` is a current Windows exception-handling workaround and is not
part of the Zod AOT design.

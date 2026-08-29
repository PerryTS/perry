# Zod `z.compile()` AOT probe

This probe exercises Perry's prototype schema-aware lowering for Zod. The
compiler recognizes a closed `z.object({...})` schema, records a compiler-only
schema IR, and replaces the matching `z.compile(schema)` call with ordinary
Perry HIR closures. Zod's generated JavaScript is not evaluated.

The generated schema clone uses a native happy path and delegates failures to
the original Zod parser. That preserves Zod's normal issue construction while
making successful parses independent of `new Function`/`dyn-eval`.

The current proof-of-concept subset is intentionally small:

- `import * as z from "zod"` or `"zod/v4"`
- `z.object()` with static, non-computed fields
- `z.string()`, `z.number()`, and `z.boolean()`
- `z.number().int()`, `.min(<number literal>)`, and `.max(<number literal>)`
- `z.compile(schema)` and the static `{ strict: <boolean> }` option

Unsupported schemas keep the regular Zod call; they are never partially
specialized.

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

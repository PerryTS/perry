# Automatic tiny programs

Perry automatically specializes a narrow class of standalone programs on native
64-bit Linux and macOS. No enabling flag is needed:

```typescript
{{#include ../../examples/runtime/tiny_program.ts}}
```

A proof over the original, unfolded AST admits only empty statements, literal
string expressions, immutable constant strings/templates, and direct calls to
the unshadowed builtin `console.log` or `console.error` with one constant string.
The evaluator keeps UTF-16 until final output encoding, including surrogate pairs
formed across template substitutions. It bounds analysis depth and stored
constant data; exceeding either bound uses normal compilation.

Every unproven construct uses the normal compiler/runtime. This includes imports,
exports, unknown calls, console aliases or mutation, computed access, functions,
classes, loops, objects, promises, timers, exceptions, and lifecycle APIs. Dead
code is checked too. A synchronous allocating loop still needs the normal GC.
Host/plugin/library modes, cross targets, source defines, custom runtime features,
and diagnostics requiring normal HIR/codegen also keep the normal pipeline.
The existing general `--no-auto-optimize` option remains respected.

The specialized executable links its constant data and a small native output
helper against the host C library. It contains no Perry runtime archive, managed
heap, GC initialization or root registration, mimalloc, class tables, promises,
or JS event-loop machinery. Empty programs need no output helper at all.
The ordinary host C compiler/linker is required, as for native linking generally.

The helper preserves byte output, final newlines, per-stream order, partial writes,
EINTR, broken pipes and ignored console write errors. Files and terminals use
synchronous writes. Pipes/sockets can fill: two stack cursors retain positions
in the immutable output table and wait for writability without blocking progress
on the other stream. This bounded I/O drain cannot execute JS callbacks or create
new work. It is necessary to preserve Node's independent stdout/stderr behavior;
removing the JS event loop does not mean ignoring backpressure.
See [Node's process I/O contract](https://nodejs.org/api/process.html#a-note-on-process-io).

Use ordinary `-v` compiler output to see the selected path or fallback reason.
`--keep-intermediates` retains `.tiny.ll`, `.tiny.o`, and the exact output helper
source/object for inspection. `PERRY_KEEP_SYMBOLS=1` retains linked symbols without
changing selection. Runtime GC diagnostics have no managed heap to observe in a
tiny executable; use normal compilation when investigating runtime behavior.

Local verification:

```sh
RUST_TEST_THREADS=1 cargo test --profile perry-dev -p perry --bin perry -- tiny_program --test-threads=1
python3 scripts/verify_tiny_program.py /absolute/path/to/perry /absolute/path/to/results
```

The verifier requires the pinned Node 26.5.1 oracle, records compiler/source/binary
hashes, executes positive and fallback programs, inspects linked symbols, and
checks independent-stream handshakes under backpressure. Run it against the actual
installed npm compiler outside a source checkout for package acceptance. Its
`--tiny-only` switch is a verifier development scope, never an enabling compiler
flag; that scope does not satisfy full fallback acceptance.

## JSON in normal programs

Direct `JSON.stringify` (including replacers and spacing), `JSON.isRawJSON`,
and `%j` formatting select serialization and its GC cache roots independently
of parsing. The parser and lazy-array materializers are reached through slots
filled by the JSON engine installer. The common root scanner visits stringify
caches directly and parser caches through an engine-installed slot; generic TLS
cache initialization remains independent of this selection.

Direct parsing, runtime JSON-file `require`, `JSON.rawJSON`, and computed
native JSON methods retain the engine. Native-module imports, UI/plugin programs, workers, and data imports
also retain it conservatively: their return adapters and callbacks can parse
JSON without a JavaScript parse call. A JSON namespace used as a value, an escaped global
object, supported dynamic code, or native-addon access retains the full JSON
namespace and engine conservatively. Worker images contribute their installers
to the process-wide union, and each thread registers cache roots during GC
initialization. This uses the existing installer symbols on native and wasm
runtime targets.

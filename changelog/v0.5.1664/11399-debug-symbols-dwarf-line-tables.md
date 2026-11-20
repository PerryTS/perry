**`--debug-symbols` now emits DWARF line tables, so debuggers can stop at `file.ts:line` breakpoints** (discussion #9856). Previously the flag handed clang `-g`, but Perry's IR carried no debug metadata, so objects had no `.debug_line` at all and lldb (Xcode, VS Code's CodeLLDB) or gdb could not bind a TypeScript breakpoint.

- Codegen (`crates/perry-codegen/src/debug_info.rs`): under `--debug-symbols`, `lower_expr` drops a `; perrydbg <line> <file>` comment for each expression that carries a source offset (calls, `new`, member reads — the offsets #5247 already resolves for error `.stack`s). Comments pass through every textual IR rewrite untouched.
- `linker::compile_ll_to_object` turns the markers into a `DICompileUnit`, a `DISubprogram` per `define`, and a `!dbg` location on each instruction. A marker-less function that directly calls a marked one (a specialization's entry wrapper, a closure trampoline) takes that callee's line, so a breakpoint on the first line of an inlined body binds on the executed path. Other marker-less functions get locations only on calls, at line 0.
- Large unit-split modules stay on the text transport under `PERRY_DEBUG_SYMBOLS`, because native C-API construction never materializes the text the pass reads.
- On `windows-msvc` targets the module flag is `CodeView` rather than `Dwarf Version`, so the line info goes into the PDB. This path is untested on Windows.
- macOS: the compiler runs `dsymutil` and writes `<exe>.dSYM` before the intermediate objects the Mach-O debug map points at are deleted.
- Default builds are unchanged: no markers are emitted without `--debug-symbols`, and the pass returns `None` for text without them.

Scope: line tables and function frames only. There is no variable or type info yet. Lines that hold only arithmetic on locals have no row of their own, so a breakpoint there slides to the next line that has code. Docs: `docs/src/cli/debugging.md`.

Validated on Linux (clang 18 text path): lldb and gdb stop at breakpoints in plain functions, in a method inlined into `main`, in a helper inlined into an `async` body, after an `await`, and inside loops.

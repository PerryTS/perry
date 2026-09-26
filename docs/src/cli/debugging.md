# Source-level Debugging (`--debug-symbols`)

`perry compile --debug-symbols` (or `PERRY_DEBUG_SYMBOLS=1`) emits a DWARF
**line table** that maps the generated machine code back to lines of your
TypeScript source. Any DWARF debugger — lldb (Xcode, VS Code's
[CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)),
gdb — can then stop at `file.ts:line` breakpoints and show where a program
is in the TypeScript source when it stops or crashes.

```bash
perry compile app.ts -o app --debug-symbols
lldb ./app
(lldb) breakpoint set --file app.ts --line 12
(lldb) run
```

On macOS the compiler also runs `dsymutil`, writing `app.dSYM` next to the
binary. Apple's linker does not copy debug info into the executable, and the
intermediate objects it would otherwise point at are deleted after linking, so
keep the `.dSYM` beside the binary (lldb finds it there by UUID).

On Windows (MSVC targets) the same line information is emitted as CodeView
into the PDB instead of DWARF. This path is not validated on Windows yet.

## Xcode

Add a build rule (or a Run Script phase) that compiles your `.ts` with
`--debug-symbols` into the product the scheme runs, e.g.

```bash
/path/to/perry compile "$SCRIPT_INPUT_FILE" -o "$BUILT_PRODUCTS_DIR/app" --debug-symbols
```

Open the `.ts` file in Xcode and click in the gutter to set breakpoints.

## VS Code (CodeLLDB)

```json
{
  "type": "lldb",
  "request": "launch",
  "name": "Debug Perry binary",
  "program": "${workspaceFolder}/app",
  "preLaunchTask": "perry: compile --debug-symbols"
}
```

Breakpoints set in the `.ts` editor bind once the binary is loaded.

## What is and is not covered

This is a first slice — **lines and function frames only**:

- Lines are recorded where an expression carries a source position — calls,
  `new`, and property reads. A line holding only arithmetic on locals has no
  code of its own in the table; debuggers slide a breakpoint on it to the next
  line that does.
- **Variables cannot be inspected** yet (no DWARF variable/type info). Stepping
  and backtraces work; `frame variable` shows nothing useful.
- Function frames are named by their generated symbol (`perry_fn_<module>__<name>`).
- The optimizer still runs, so stepping can jump between lines the way it does
  in any optimized native build.

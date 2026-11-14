# Function Order (`--record-function-order`, `--function-order`)

A compiled program's machine code is laid out in the order the compiler
emitted it, which has nothing to do with when the code runs. A large program
that runs a few thousand functions while it starts touches pages spread over
its whole text, and each first touch reads a whole chunk of the file into
memory. Laying out the functions that run first, together and in the order
they first run, keeps far less of the binary resident and needs fewer page
faults.

The order comes from one real run, in two steps.

## 1. Record

```sh
perry compile src/main.ts -o app-rec --record-function-order
PERRY_FUNCTION_ORDER_OUT=app.fnorder ./app-rec    # use the app as usual
```

The recording build calls a small hook the first time each compiled function
runs, and the hook appends the function's name to the file named by
`PERRY_FUNCTION_ORDER_OUT`: one name per line, in first-execution order. Each
name is written as soon as the function first runs, so stopping the program
at any point (even with `kill -9`) keeps everything recorded up to then. The
file is appended to, so delete it before a fresh recording. Without
`PERRY_FUNCTION_ORDER_OUT` the recording build records nothing.

Record the part of the program whose memory and start-up time matter: up to
the first screen for a CLI or TUI, or through a typical request for a server.
The recording build is a little slower; don't ship it.

## 2. Build with the list

```sh
perry compile src/main.ts -o app --function-order app.fnorder
```

The listed functions are placed first in the text, together, in list order.
Every other function keeps its usual place. The list only changes where code
sits, never what it does: names the program doesn't define are ignored and
missing names fall back to the usual order, so a list recorded from an older
version of the program is harmless. It just orders less.

Normal builds carry no recording code; `--function-order` costs nothing at
run time on non-Linux targets. Ordered Linux executables make one bounded
backing call at startup: prefixes smaller than 2 MiB return immediately, and
only complete selected 2 MiB pages are eligible for huge-page backing. The
linker keeps ordinary ELF alignment and does not pad the selected prefix.
After PIE relocation, backing covers only complete aligned pages inside
selected code; partial boundary pages retain their original file mapping.

## The runtime's default order

Without `--function-order`, a Linux build still orders the runtime: the
compiler carries a list of the runtime functions that ordinary programs
execute, the most widely used first, and the link places those at the front
of the text. Only about a quarter of the linked runtime code ever runs, and
unordered it is spread over nearly the whole text, so most of the binary ends
up resident; with the order the pages a program touches are packed together.
The program's own code and its instructions are unchanged, and nothing is
added to the binary: only the layout differs.

- `PERRY_HOT_TEXT_ORDER=off` keeps the unordered layout.
- `--function-order` (a recording of your program) replaces the default order.
- The list is `crates/perry/src/commands/compile/link/hot_text_order.txt`.
  `scripts/hot_text_order.py build` makes it from recordings of a corpus of
  programs, and `scripts/hot_text_order.py check` fails when it no longer
  matches the runtime (a listed function was renamed, or a smoke program
  executes too much code outside the ordered run).
- GNU ld and lld on Linux by default. On macOS / iOS / tvOS / watchOS /
  visionOS the same list can be passed to ld64 (or ld64.lld) as an
  `-order_file` of exact symbol names resolved from the runtime archives:
  opt-in with `PERRY_HOT_TEXT_ORDER=on`, because its effect on Apple's 16 KiB
  pages has not been measured yet. Windows, WebAssembly, Android and
  HarmonyOS builds are unchanged.

The same link script carries a default order for the runtime's read-only
data: the `.rodata` and `.data.rel.ro` sections (tables, vtables, constant
strings) that ordinary programs read are placed at the front of their output
sections, so the data pages a program touches are packed as well.

- `PERRY_HOT_DATA_ORDER=off` keeps the previous data layout (the text order
  has its own switch).
- The list is `crates/perry/src/commands/compile/link/hot_data_order.txt`, one
  hash-free key per data section: its symbol, or for an unnamed constant a
  digest of its bytes and relocations. `scripts/hot_data_order.py` records it
  (valgrind's lackey, every load) and builds it, and its `check` command is
  the gate, as for the text list.
- Native glibc Linux executables only. With GNU ld the `.rodata` half needs
  x86-64's default script (`-z separate-code`); it adds at most one page of
  alignment padding in front of `.eh_frame_hdr`. Merged sections
  (`.rodata.str1.1`, `.rodata.cst16`) are pooled by the linker and not
  ordered.

## Platforms

| Platform | How the order is applied |
|---|---|
| Linux (GNU ld, the default `cc`) | The linker script collects ranked generated sections and native runtime function sections at the start of `.text`, with ordinary ELF alignment. |
| Linux/Android with lld | The same sections plus `--symbol-ordering-file`. |
| macOS / iOS (ld64) | `-order_file` with the list (Mach-O names add the leading `_`). |
| Windows, WebAssembly | Not applied; the build is unchanged. |

## The list format

Plain text, one function symbol per line, as the recording build writes it.
Blank lines and lines starting with `#` are ignored; if a name appears twice,
its first position counts. You can edit the list by hand, for example to
merge two recordings.

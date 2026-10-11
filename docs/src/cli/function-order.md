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

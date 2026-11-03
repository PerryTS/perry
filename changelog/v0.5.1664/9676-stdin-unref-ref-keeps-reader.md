### Fixed

- A TUI no longer goes permanently deaf to the keyboard after `process.stdin`
  is `unref()`d and `ref()`d again (#9676). On the stdin *object* path — an
  alias, a parameter, or a destructured field, which is what ink and every TUI
  built on it use — `unref` was wired to the same detach stub as
  `pause`/`destroy`: it set a process-global latch, and the runtime's fd-0
  reader thread breaks its loop on that latch and exits. `ref` was wired to a
  no-op stub, so nothing ever cleared the latch or restarted the reader. One
  `unref()`/`ref()` pair therefore left the process with no reader on fd 0 for
  the rest of its life: the event loop kept ticking, the terminal stayed in raw
  mode, the process still woke on each keystroke, and not one further byte
  reached JS. Ink performs exactly that pair whenever its raw-mode refcount
  drops to zero and comes back — i.e. whenever the last `useInput` component
  unmounts and a new one mounts, which is what a tool call does — so this is
  the long-standing "TUI input dies after a minute of real use" symptom.
  `ref`/`unref` now govern only the event-loop hold, as in Node: an unref'd
  stdin keeps delivering `'data'`, and `ref()` restores the hold. Only an
  explicit `pause()`/`destroy()` stops the reader, and `resume()` still clears
  both.

- The `process.stdin` object's `pause()`/`resume()` now reach the same flow
  state as codegen's literal `process.stdin.pause()`/`.resume()` spelling
  (#9676). `rl.close()` and a literal `pause()` set perry-stdlib readline's
  `STDIN_PAUSED`, whose pump branch deliberately leaves `PENDING_DATA`
  undrained — while readline's fd-0 reader keeps reading and keeps waking the
  main thread. Only the literal `process.stdin.resume()` could clear that flag,
  so a TUI that holds stdin in a variable (`const s = process.stdin; …
  s.resume()`) and opens a single readline prompt went permanently deaf: bytes
  still consumed off the terminal, CPU still burnt on every keystroke, nothing
  ever dispatched to JS. The two spellings are now bridged the same way `on`
  and `off` already were.

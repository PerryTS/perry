`Bun.plugin(...)` no longer prints a stub warning to stderr when it registers.
Registration is not observable in a native build: no JavaScript module loads at
run time, so no hook can run, and a run-time import that a hook would have
served already rejects with an error naming the AOT boundary and the inert
hooks. The warning reached the terminal of every program that registers a
plugin at startup; opencode's TUI painted it over its first frame.

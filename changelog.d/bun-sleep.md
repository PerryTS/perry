`Bun.sleep(ms | Date)` returns a Promise that resolves `undefined` after the
delay (or at the Date; a past Date or negative delay resolves on a later turn),
and `Bun.sleepSync(ms)` blocks; both are also callable values on `Bun` and the
`bun` module. They threw "Bun.sleep is not supported by Perry"; opencode's TUI
awaits `Bun.sleep` in its migration overlay.

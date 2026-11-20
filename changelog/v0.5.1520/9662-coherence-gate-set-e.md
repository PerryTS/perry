- **Fixed two `set -e` traps in the new tokio-coherence gate.** The gate added
  alongside the unified ext build could never run to completion: GitHub executes
  `run:` steps with `bash -e`, and the step had two constructs that exit under it.

  1. `[ -z "$got" ] && continue` returns 1 whenever `$got` is **non-empty** — the
     normal case, an archive that *does* bundle tokio — so `-e` killed the step
     there. Now a plain `if`.
  2. `tokio_of()` is a pipeline ending in `grep`, which exits 1 when an archive
     bundles no tokio. With `pipefail` the pipeline carries that status,
     `got=$(tokio_of …)` inherits it, and `-e` killed the step on the first
     non-tokio archive. Now `|| true`.

  In run 33940039247 the macOS aarch64 leg failed at this step having printed
  only `stdlib bundles tokio-7be87cf38f2c1f6e` and compared **nothing** — the
  build itself was fine.

  The original sabotage test ran `set -uo pipefail` **without** `-e`, which is
  exactly why it passed locally and failed in CI. The test now runs under
  `bash -e`: incoherent → fail, coherent → pass with `checked > 0`,
  nothing-compared → fail.

- **Measured: the unified ext build works.** On macOS aarch64, *Build native ext
  libraries (Unix)* went from **304 min to 12 min**, and the leg's real work from
  **291 min** to ~30 (perry 6 + runtime 9 + panic-abort 3 + ext 12). The log
  confirms `built 40 ext packages in a single cargo invocation`.

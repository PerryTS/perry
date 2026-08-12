# CI gate repair: #7977, #7971, #7970

Working notes. Written incrementally; the PR body is the summary.

---

## #7977 — `windows-build` red before it runs anything  [FIXED]

### What this gate covers (and what was therefore unprotected)

`windows-build` is the **only Windows execution of anything** in per-PR CI. Its
step list, in order:

1. GC structural audits (Windows)  <- died here, 27 s in
2. Install Rust toolchain / LLVM 22 / Node
3. Build compiler + runtime + `perry-ui-windows` + `perry-ui-windows-winui`
4. **`perry-runtime` unit tests** (`RUST_TEST_THREADS=1`, #7356)  <- the big one
5. Windows parity harness smoke
6. `perry.exe` VERSIONINFO resource check
7. COFF duplicate-symbol archive trimming test

Steps 2-7 were `skipped` on **every** PR whose Windows run executed the current
`test.yml`. So for the duration: **no Windows compile of the runtime, the stdlib
or either Windows UI crate; no Windows run of the `perry-runtime` unit tests; no
Windows parity smoke; no VERSIONINFO check; no COFF-trimming test.** A Windows-only
miscompile, a Windows-only test failure, or a broken `perry-ui-windows` build would
all have merged green-by-skipping.

### Root cause (confirmed to the byte)

`scripts/check_thread_locals.py` reads Rust sources with a bare
`Path.read_text()`, which decodes with `locale.getencoding()` — **cp1252** on a
GitHub Windows runner. 15 files under `crates/perry-runtime/src` carry a byte
cp1252 has no mapping for (0x81/0x8d/0x8f/0x90/0x9d). Reproduced locally:

```
15 files undecodable as cp1252
   crates/perry-runtime/src/i18n.rs                 offset 31552  0x8d
   crates/perry-runtime/src/intl/duration_format.rs offset 17349  0x81
   ...
i18n.rs: offset=31552 newlines_before=923 crlf_position=32475   (issue says 32475)
```

The issue's arithmetic is exact: `core.autocrlf` widens 923 `\n` to `\r\n`, so
31552 + 923 = **32475**, the position in the traceback.

`#7882` ("make GC structural audits portable") fixed the *path-separator* half of
this class in this same file and the *encoding* in `gc_runtime_root_holders.py`,
but missed these four readers. Same shape, one file over.

### Changed

- `scripts/check_thread_locals.py` — all 5 reads and 15 writes now go through
  `read_source()` / `write_source()` helpers that pass `encoding="utf-8"`
  (and `newline=""` on write, so `--update` is byte-stable across hosts).
  The gated content is **unchanged**: the `files` map the checker verifies is
  identical before and after (asserted, see validation).
- `scripts/gc_runtime_root_holders.py` — one **latent** instance of the same
  class in the self-test's fixture writer. ASCII today, so it has not bitten;
  fixed with the rest.
- `tests/test_gc_ratchet.py:1369` — bare `read_text()` on the shipped baseline.
- `scripts/check_locale_independent_io.py` — **new gate** (below).
- `.github/workflows/test.yml` — new `lint` step running that gate;
  `PYTHONUTF8: "1"` on the Windows audit step as belt-and-braces.

### The new gate, and why it is static

`scripts/check_locale_independent_io.py` AST-scans exactly the six Python files
`windows-build`'s audit step executes, and fails on `open()` / `read_text()` /
`write_text()` / `Path.open()` without an explicit `encoding=` (binary modes
exempt).

Static, not `PYTHONWARNDEFAULTENCODING`, for two reasons:

- `EncodingWarning` only fires on a call that **executes**. A bare `read_text()`
  on an error path or in a subcommand CI does not invoke warns nobody and ships.
- `tests/test_gc_ratchet.py` writes Python probes as *string literals* containing
  `open(...)`. Those are not calls; the AST correctly ignores them, a grep would not.

It runs in **`lint`** — Linux, per-PR, and a **required** context — because the
defect is invisible on Linux at runtime. That is the point: the class is now
caught before it can reach Windows, rather than by taking the Windows job down.

### Validation

| check | result |
|---|---|
| reproduce main's failure under a non-UTF-8 locale | `UnicodeDecodeError: 'ascii' codec can't decode byte 0xe2`, exit **1** |
| same command, fixed | exit **0**, `thread-local policy OK: 173 hot declarations, 129 raw blocks in 91 recorded cold files` |
| full 8-command Windows audit sequence, non-UTF-8 locale | all **PASS** |
| `--update` output vs shipped allowlist | `files` map **identical** |
| **sabotage**: replant the exact #7977 defect | new gate exits **1** and names `check_thread_locals.py:115` |
| remove the replant | new gate exits **0** |
| new gate `--self-test` | OK — 6 flagged shapes, accepted shapes clean, scope asserted |

The sabotage arm is the one that matters: a green run of this gate means the
detector works, not that nothing was tried.

### Not changed (deliberate)

- `_hot_declarations` in `thread_local_cold_allowlist.json` reads 163 against an
  actual 173. **Pre-existing drift on `main`, not caused by this change**, and
  `verify()` does not gate on that field — only on `files`. Left alone to keep
  the diff reviewable; worth a one-line `--update` in a separate change.
- `gc_runtime_root_holders.py` raises `UnicodeEncodeError` when *printing* an
  em-dash under an ASCII locale. Measured: its output **is** cp1252-encodable, so
  this is an artifact of the stricter ASCII simulation and **not** a Windows
  defect. `PYTHONUTF8=1` on the step covers it regardless.

### Promotion

Nothing promoted. `lint` is **already** a required context; this adds a step to it.
That is deliberate — CLAUDE.md hazard 2 is the step people forget, so the gate is
placed where that step does not exist. It is green locally on this tree.

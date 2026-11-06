# tests/parity/ — conformance gating artifacts

`scripts/parity.py` runs every program case and judges it against the
snapshots in this directory. The node-suite cases and committed expected
outputs live here too.

## Where runtime regression tests belong

A new program case goes under the subject whose behavior it observes:
`node-suite/<module>/` for one Node core module, `language/<area>/` for
JavaScript semantics, `typescript/<feature>/` for TypeScript-only syntax,
`perry/<api>/` for Perry-only APIs and `npm/<package>/` for a compiled package
(`docs/src/testing/test-structure.md`). `tests/fixtures/programs/test_gap_*` takes no new
files; `lint` refuses one.

Each subject tree is discovered recursively. Runnable formats are `.ts`,
`.cts`, `.mts` and `test_*.cjs`; each must be independently runnable. Keep the
extension when moving a case so Node retains its module mode. Case ids omit
TypeScript-family suffixes but retain `.cjs`. Helpers go in a `fixtures/`
directory, which the runner skips in every format.
Legacy helper entry cases keep their standalone subject paths until their
coverage is explicitly replaced. Preserve their `.ts` compile-smoke enrollment
in `compile-smoke.txt`; fixture exclusion must not silently remove a gate.

When Perry intentionally differs from Node, commit the Perry output as
`tests/parity/expected/<test-name>.txt`; add an `expected-exit` file when the
expected Perry exit is nonzero. Keep a shell harness only when the assertion
requires behavior the parity runner cannot express, such as inspecting
compiler output, separating stdout from stderr, creating native libraries,
driving a background service, enforcing a hard timeout or liveness contract,
testing symlink layout, or running multiple compiler/GC configurations.

## Snapshots

The `gap-suite` job of `test.yml` runs every `test_gap_*` case in `tests/fixtures/programs/`
and all subject trees in every core PR tier in `fast` mode through
`scripts/parity.py run --suite all --filter test_gap_ --mode fast`, judged by
`snapshots/linux-fast/<subject>.json`. Keep the `test_gap_` basename when
moving a case into its subject so it stays in this universal lane. The `full` tier's `parity` job runs
every case of `tests/fixtures/programs/` and `node-suite/` in auto-optimize mode, judged by
`snapshots/linux-auto/` — see `docs/src/testing/ci-tiers.md` and
`docs/src/testing/test-structure.md`.

A snapshot lists every case that is **not passing**. A case absent from it is
expected to pass. The check is **bidirectional**: a new failure is red, and so
is a listed case that starts passing. That second direction is the whole
reason the file exists: a one-directional list keeps a fixed test's entry
forever and decays into orphans nobody can revalidate (#797).

Never hand-edit statuses. Re-baseline from CI (Linux, `fast` mode) as
`docs/src/testing/ci-tiers.md` "Re-baselining the gap snapshot" describes, or
locally against your own build:

```bash
npm ci --ignore-scripts --no-audit --no-fund
python3 scripts/parity.py run --suite all --filter test_gap_ --mode fast --update
```

The runner refuses to run unless `node --version` exactly matches
`.node-version` and the root npm dependency set is installed. Both are oracle
inputs: a missing package makes Node print `ERR_MODULE_NOT_FOUND` while Perry
can still compile its compatibility implementation, manufacturing a
"regression" that no compiler change caused (#9273).

A new entry lands as an untriaged placeholder; add its issue, date, category,
and reason before committing. A crash is never accepted into a snapshot.

Snapshots live at `snapshots/<platform>-<mode>/<subject>.json`, so a
platform-only result never moves Linux's ratchet. A host without its own file
compares every case against "must pass"; establish one with `--update` on
that host, triage it, and commit it.

The snapshot records `node_fail` and `skipped` explicitly instead of dropping
them. A test the oracle stopped covering is a visible diff, not a silent hole:
CI sat on Node 22 while the suite grew Node 24/26 features and hid 14 tests
that way (#6364). The oracle version is pinned in `.node-version` — it is a
correctness input, so regenerate the snapshot on the pinned Node, on Linux
(several failure classes are platform-specific).

## Case headers

`scripts/parity.py` reads `// parity-<key>: <value>` comment lines from a case:
`argv`, `node-argv`, `env` (space-separated `KEY=VALUE`), `skip` and
`rlimit-as-mb`.

The full sweep executes a source file only when it is a self-contained,
terminating program whose output can be compared with Node (or with a committed
expected-output file). A fixture that instead needs terminal input, a client to
drive a background server, or an external service must declare a non-empty
`// parity-skip: <reason>` header. The runner records it as `skipped`; compile
smoke and the fixture's lifecycle-specific harness remain responsible for its
coverage. Keep the reason concrete and name that harness when one exists.

### Triage fields (#797)

An entry carries `issue`, `added`, `category` and `reason` beside its
`status`. `--update` writes a new entry as `untriaged` with today's date and
keeps the fields of an entry that already exists; the runner does not validate
them, so review is what keeps them honest. `issue` is a GitHub issue number as
a string, `added` an ISO `YYYY-MM-DD` date, and `category` one of the values in
the table below. When the tracking issue closes but the test still fails,
re-triage: mark the entry `bug-stale` and file a new tracking issue.

A failure specific to one operating system belongs in that platform's snapshot
file, which only a run on that platform reads.

The runner also works on Windows. `cargo`, the pinned Node.js, Python and the
Windows linker toolchain must be on `PATH`.

## Category definitions

The `category` field of a snapshot entry.

| Category | Meaning |
|---|---|
| `ci-env` | Passes locally, fails in CI for environmental reasons (missing fixture server, no network). Not a Perry defect. |
| `module-inventory` | A Node module surface that is not fully implemented yet. Flips to pass as APIs land. Not a regression. |
| `bug-open` | A real Perry defect with an open tracking issue. |
| `bug-stale` | The tracking issue is closed but the test still fails. Needs re-triage and a new issue. |
| `gap-categorical` | A known categorical gap (e.g. `console.dir` formatting, lone surrogates). See CLAUDE.md. |
| `gap-bisect` | A failure whose cause has not been localized yet; needs a bisect. |
| `untriaged` | Machine-written placeholder from a snapshot update. Replace it with a real category and issue. |

## Generated, gitignored

`reports/` and `output/` are per-run artifacts and are gitignored. The
committed evidence that gates a run is `snapshots/<platform>-<mode>/`.

# Node 26 everywhere — inventory, verdicts, and what must be regenerated

Status: COMPLETE. PR https://github.com/PerryTS/perry/pull/7967

Owner directive: "We want Node 26 everywhere." Project oracle is **26.5.1**
(`.node-version`, authoritative).

## Verified inventory (all read from `git show origin/main:<path>`, not a working tree)

| location | value on origin/main | verdict |
|---|---|---|
| `.node-version` | `26.5.1` | correct, authoritative |
| `external-tools.json` `tools.node.version` | `26.5.1` | correct |
| `benchmarks/public-baseline-config.json` `toolchains.node` | `v22.23.1` | TBD (main target) |
| `test-compat/node-core/pinned-version.txt` | `v22.x` | LEAVE — Node's own corpus must match its own line |
| `.github/workflows/npm-launcher.yml` | `"22.23.1"` x2 | TBD |
| `.github/workflows/release-hono-server.yml` | `"24"` | TBD |
| `.github/workflows/release-packages.yml` | `"20"` | TBD |

Correction to the handed-down inventory: `npm-launcher.yml` is **not** a bare
`22` — it is `node-version: "22.23.1"` in **two** jobs (lines 36 and 55).

## Notes log

### FINDING 1 (load-bearing) — the benchmark config cannot be changed on its own

`benchmarks/public-baseline-config.json` is listed in `public_baseline.py`'s
`HARNESS_PATHS`, so its bytes feed `freshness.harness_fingerprint`. Two
independent hard checks in `validate_public()` fire on a node-pin edit:

1. `harness_fingerprint` mismatch -> `"public artifact benchmark harness changed;
   regenerate it with ./benchmarks/run_public_baseline.sh"`.
2. `for runtime, expected_version in measurement_config["toolchains"].items()`
   compares the config against `artifact["runtimes"][runtime]["version"]`, which
   is baked as `v22.23.1` in `benchmarks/results/public-node-bun-v1.json`. No
   migration tuple can bypass this one.

Measured, not assumed (edit applied, checker run, file restored in one step):

```
$ (node pin -> v26.5.1) python3 benchmarks/ci_public_baseline_check.py
EXIT: 2
public baseline error: public artifact benchmark harness changed;
regenerate it with ./benchmarks/run_public_baseline.sh
```
Unmodified tree: exit 0.

`benchmarks/ci_public_baseline_check.py` runs as the "Public benchmark evidence
freshness" step of the **`lint`** job (`.github/workflows/test.yml:88` job,
step at :137). `lint` is a REQUIRED context (CLAUDE.md "Workflow Requirements").

=> Editing the node pin without regenerating turns a required gate red for main
and for every open PR. The config change and the regeneration are **atomic by
design** (#7282/#7958). Shipping the pin edit alone is not "stale but honest",
it is a broken required gate — the exact failure CLAUDE.md's GATE THEATRE note
warns about.

The artifact also currently passes only via `_HARNESS_FINGERPRINT_MIGRATION`
(it records the pre-#7282 digest `28117b86...`), so there is no spare slack.

### FINDING 2 — the unaccounted-for workflow is `npm-launcher.yml`, and it is drift

`.github/workflows/npm-launcher.yml` pins `node-version: "22.23.1"` twice
(lines 36, 55) with **no exemption comment**. Provenance:

* `4f50d797c` (**2026-07-13**) created the file with the literal `22.23.1` —
  PR #6350, the glibc-2.35 launcher fix for #6298.
* `db4b068d8` (**2026-07-13**, the SAME DAY) is #6367, the standardization that
  introduced `.node-version` (26.5.0) and converted every workflow to
  `node-version-file:`. Its commit message enumerates the exemptions it granted:
  `node-core-subset.yml`, `release-hono-server.yml` (24), `release-packages.yml`
  (20). **`npm-launcher.yml` is not among them** — it did not exist when #6367
  was written and landed alongside it.

So `22.23.1` is not a decision; it is the ambient Node of the day, frozen. It is
also exactly the value still sitting in `benchmarks/public-baseline-config.json`,
which is the same fossil.

Behaviour-sensitivity: the `detect-self-test` job runs
`node npm/perry/test/detect.test.cjs`, which exercises `npm/perry/bin/detect.cjs`
— the platform-resolution logic every installing user hits. That is JS behaviour
under test, not a publishing toolchain. It should track the oracle.

Blocker check for the `ubuntu-22.04` job (glibc 2.35): Node 26 requires
**glibc >= 2.28** (nodejs/node `v26.x` BUILDING.md support matrix, Tier 1 for
linux x64 and arm64), so 2.35 is comfortably above the floor. No blocker.

VERDICT: both occurrences -> `node-version-file: .node-version`.

### FINDING 3 — the two release workflows

Node release schedule (nodejs/Release `schedule.json`, fetched):
`v20` EOL **2026-04-30** (today is 2026-08-12 -> EOL), `v22` EOL 2027-04-30,
`v24` EOL 2028-04-30, `v26` start 2026-05-05, LTS 2026-10-28, EOL 2029-04-30.

* `release-packages.yml` = `"20"`: an **end-of-life** runtime in the repo's most
  privileged job (`id-token: write`, OIDC-publishes every platform package).
  That is a defect independent of the directive. -> **26**.
* `release-hono-server.yml` = `"24"`: supported, chosen for npm >= 11.5.1.
  Bumping is consistency only, and is safe (Node 26 ships npm 11.x). -> **26**.

Both keep their `npm install -g npm@latest` step (Node 26 already satisfies the
OIDC floor, but the step is free insurance) and both keep an exemption comment:
they are pinned to a **major literal**, deliberately NOT `node-version-file`,
because a gap-suite oracle bump must not be able to move a publishing toolchain.

### FINDING 4 — stale instruction inside `external-tools.json`

`tools.node.notes[0]` says "Bump via the NODE_PIN.version constant in
scripts/node_compat_matrix.mts". There is **no `NODE_PIN` constant** in that
file (grep: zero hits); the script reads `external-tools.json` itself and its
own `--help` says so ("The pinned Node version lives in external-tools.json").
CLAUDE.md gives the correct instruction. Fixed the note in place.

### FINDING 5 — CLAUDE.md said "Two workflows are deliberately exempt", then listed three

Verbatim on origin/main (line 20): *"**Two** workflows are deliberately exempt
and say so inline: `node-core-subset.yml` ... and **the two release
workflows**"* — 1 + 2 = 3. That undercount is the most likely reason the fourth
pin (`npm-launcher.yml`) read as accounted-for when it never was. Corrected, and
the prose now points at the checker instead of trying to be the registry.

### What was changed, and what was not

CHANGED
* `.github/workflows/npm-launcher.yml` (x2): `"22.23.1"` -> `node-version-file: .node-version`.
* `.github/workflows/release-packages.yml`: `"20"` -> `"26"` (Node 20 is EOL).
* `.github/workflows/release-hono-server.yml`: `"24"` -> `"26"`.
* `external-tools.json`: removed the stale "NODE_PIN constant" bump instruction.
* `CLAUDE.md`: "Two" -> "Three"; points at the checker; states the benchmark pin.
* NEW `scripts/check_node_version_consistency.py` + a `lint` step in `test.yml`.

LEFT ALONE, with reasons
* `.node-version` (26.5.1), `external-tools.json` (26.5.1) — already correct.
* `test-compat/node-core/pinned-version.txt` (`v22.x`) — Node's own corpus must
  be run by its own line. Registered exemption.
* `benchmarks/public-baseline-config.json` (`v22.23.1`) — see FINDING 1: not
  editable without a 2 h regeneration, which this host cannot perform.
  Registered as a self-clearing exemption.
* `benchmarks/results/public-node-bun-v1.json`,
  `benchmarks/honest_bench/results/metadata.json` — measurement OUTPUTS, already
  tied to the config by `public_baseline.validate_public`. A second checker over
  them would be a place for the two policies to disagree.
* `bun: 1.3.14` in the same config — OUT OF SCOPE per the brief, and it is *not*
  equally stale: 1.3.x is the current Bun line (Node 22 is four majors behind;
  Bun 1.3 is not). It is frozen by the same atomic-regeneration rule, so if the
  baseline is regenerated it should be re-pinned to current Bun in that same run.
* `package-lock.json` / vendored fixture lockfiles (`"node": ">=20"` etc.) —
  dependency `engines` floors declared by third-party packages, not pins Perry
  chooses. Changing them would be fiction.

### Does the published comparison move against us?

Direction: **yes, against us.** Node 26 ships a materially newer V8 than Node
22, so Perry's published "x times faster than Node" ratios should shrink. That
is the honest direction and the reason to do it.

Magnitude: **not measurable on this host, and deliberately not guessed.** See
the regeneration section — the artifact's own policy (<=25% CPU active for 60
consecutive seconds, re-checked before each of five components) is the project's
own statement of what a trustworthy number costs, and this box has run at load
30-200 all day with several concurrent agent builds. Any number produced here
would be junk wearing a decimal point.

### Regeneration runbook (what the next person must do)

Host: `perry@perry-macos.local` — the Apple M1 / 8-core / 8 GB mini recorded in
the artifact's `host` block. It must be that machine: the artifact pins host
identity, so regenerating elsewhere replaces the baseline rather than updating
it, and no before/after ratio would be comparable.

1. Quiesce the mini (no agent builds, no other benchmarks).
2. Install Node 26.5.1 and confirm `node --version` matches `.node-version`.
3. Edit `benchmarks/public-baseline-config.json`: `"node": "v26.5.1"`
   (and consider re-pinning `bun` in the same run — same atomicity applies).
4. `./benchmarks/run_public_baseline.sh` (~2 h; five components, each gated on
   the quiet-host check).
5. `python3 benchmarks/ci_public_baseline_check.py` must print OK.
6. Delete the `benchmarks/public-baseline-config.json` entry from
   `scripts/check_node_version_consistency.py` — leaving it stale FAILS the
   check, which is what makes this exemption self-clearing rather than a fossil.
7. Expect the headline ratios to drop. Publish the new ones.

### Incidental: do not run `scripts/check_gate_freshness.py` locally

It is not a read-only checker — it calls the GitHub API and opens/updates a
sticky issue. Running it during this task opened
https://github.com/PerryTS/perry/issues/7966 ("CI gate freshness alert: 11
gate(s)..."). The content is accurate and the issue is self-closing and updated
in place by `gate-freshness.yml`, so it was left open rather than suppressed.

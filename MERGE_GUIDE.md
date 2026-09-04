# MERGE_GUIDE.md — auditing and landing the open-PR queue

How a session picks up the PR queue, audits it honestly, and lands it. Written
from the trains that produced v0.5.1512→v0.5.1520; every trap below cost real
time at least once.

Companion docs: `CLAUDE.md` (architecture, gates, GC knobs),
`docs/src/testing/ci-tiers.md` (what CI runs where).

## 0. The one-paragraph version

Cherry-pick the open PRs onto one train branch, fix whatever gates the train
breaks, validate **once** as a tree, and land the whole train as a single
own-branch PR merged with `gh pr merge --rebase --admin`. Rebase-merge keeps
each cherry-pick's authorship. Then prove main is byte-identical to what you
validated: `git diff origin/main train<N> --stat` must be empty.

## 1. Working tree

Use a dedicated worktree with its own target dir — never the user's checkout,
never another session's:

```bash
cd /Users/amlug/agent-a3                       # this session's worktree
export CARGO_TARGET_DIR=/Users/amlug/agent-targets/val
```

Rules that are not negotiable:

- **Never `git stash`** in a worktree — the stash is shared across all of them.
- **Never `git reset --hard`** or a broad clean; other sessions have live work.
- **Keep the `-p` package set identical** across every build in an A/B or a
  bisect. Dropping `-p perry` changes cargo feature unification and silently
  changes what you measured.
- Before deleting any worktree or target pool, require an OWNER file and
  announce it. A process check cannot prove a target dir idle.

## 2. Triage

```bash
gh pr list --state open --limit 40 --json number,title,isDraft,additions,deletions \
  --jq '.[] | select(.isDraft|not) | "#\(.number) +\(.additions)/-\(.deletions) \(.title)"'
```

Drafts are excluded, with one caveat: **a pushed draft is a landing candidate
within minutes** here. Re-check `gh pr view <n>` state before every push.

Fetch each head to a local ref:

```bash
git fetch -q origin refs/pull/<n>/head:t<n> --force
```

**`--force` matters.** Authors push updates mid-audit. A stale `t<n>` lands the
wrong code silently — the only tell is the diffstat drifting from what
`gh pr list` reports. Re-fetch immediately before building the train, and
compare `git rev-list --count origin/main..t<n>` against the PR's commit count.

## 3. Auditing a PR

Read the diff, not the PR body. The body says what the author meant; the diff
says what ships. What to check, in rough order of how often it finds something:

**Does the change's own test actually exercise the subject?**
A gate that runs but whose subject never did is the most dangerous kind,
because the job is genuinely green. Assert the subject was live
(`copied_objects > 0`, a counter moved, the fixture starts in the proven
state), not merely that nothing threw.

**Which direction does the analysis fail in?**
For any scanner, ratchet, or static proof, ask what an *unrecognized* input
does. Unknown must mean unsafe. A catch-all `_ => {}` that also stops
descending into children is the classic hole — check that the child walk runs
unconditionally after the match, and that the walker it delegates to is
exhaustive (no `_ =>`, so a new variant breaks the build).

**Is a regex/string-scan test vacuous on a reword?**
`!source.contains("fn js_foo(")` is a tripwire, not a proof. Fine as a
tripwire; never accept it as evidence the mechanism works.

**Rooting (the expensive class).**
A GC value's root store must dominate every later site that can collect. In
runtime code that means: no bare Rust local holding a heap pointer across a
call that allocates. The idioms are `with_{mut,const}_ptr` (scoped argument to
a non-allocating callee) and `across_{mut,const,nanbox}` (callee can collect —
re-read after). An empty-closure `across_nanbox(|| ())` is gaming the ratchet,
not a fix. Note the shape of the bug: an unrooted *register* is intermittent;
an unrooted *cache or side table* fails at collection #0 and stays failed, so a
perfectly reproducible GC bug points at a table.

**Published ABI.**
Tightening the contract of a `#[no_mangle] pub extern "C"` symbol in place is a
break for FFI and separately-loaded provider images even when the tree
compiles. The fix is a second symbol, not a stricter comment.

**Cross-platform.**
libc signatures differ (`openpty`'s trailing params are `*mut` on macOS and
`*const` on Linux). An address floor that looks fine on macOS can admit real
Linux addresses — use `is_above_handle_band` / `HANDLE_BAND_MAX`, never a bare
`>= 0x10000`.

**Version metadata.** External contributors must not touch
`[workspace.package] version` in `Cargo.toml` or `**Current Version:**` in
`CLAUDE.md`. Check and strip:

```bash
git diff origin/main train<N> -- Cargo.toml CLAUDE.md | grep -cE '^\+.*(^version|Current Version)'   # want 0
```

**Changelog.** Every PR needs `changelog.d/<PR>-<slug>.md`. Never append to
`CHANGELOG.md` (frozen at v0.5.1264). Write the fragment yourself if it is
missing.

### When to hold rather than land

Hold when the failure mode is **silent** and the shipped tests only prove
mechanics. The examples that earned this rule: a refactor that makes an
extension registry the sole seam for event pumps (failure = a server that
accepts connections and never dispatches, i.e. a hang, not an error), and
anything that removes a redundant path so a hole has no backup. Those need an
end-to-end run, not a unit test. Holding is not blocking — say what evidence
would clear it, and get that evidence yourself if you can.

Do **not** hold for style, for a non-blocking follow-up, or for CI. Note those
in the train PR body and move on.

## 4. Building the train

```bash
git checkout -q -B train<N> origin/main
for pr in 9697 9698 9699; do
  git cherry-pick origin/main..t$pr || { echo "CONFLICT #$pr"; git cherry-pick --abort; }
done
```

Pick the **range** (`origin/main..t<n>`), not the tip commit — PRs often carry
several commits, and picking the tip drops the rest.

`git merge-tree` reporting no conflict does **not** mean the cherry-pick is
clean: merge-tree models a merge, a cherry-pick is a rebase. Trust the real pick.

### Resolving conflicts in Rust

Automated resolvers mangle Rust in four distinct ways. Resolve by hand, by shape:

| shape | resolution |
|---|---|
| entry lists (allowlists, JSON gate registries) | sorted union |
| ordinary code hunks | concatenate both sides — never dedupe |
| match-arm alternations (`A \| B =>`) | hand-merge; a sorted union corrupts them |
| a conflict cutting through a struct literal | take one side whole; concatenation gives "field specified more than once" |

Never dedupe a closing `}` that legitimately appears on both sides. When two
sides both define a signature, read the authoring PR's own file to see which is
right. Regenerate generated docs (`scripts/regen_api_docs.sh`) rather than
merging them — `docs/api/perry.d.ts` and `docs/src/api/reference.md` say "do
not edit by hand" and mean it.

### If you fix something the author should have fixed

Write the fix as its own commit on the train so authorship stays clean. But
**check whether the author already pushed one** — convergence is common, and
their version is usually the better idiom and is theirs to own. Drop yours.

## 5. Validating

Cheap checks first, so a bad train dies in seconds:

```bash
cargo fmt --all -- --check
./scripts/check_file_size.sh          # 2000-line cap
python3 scripts/raw_handle_debt.py    # bare + --no-raise-vs origin/main + --self-test
```

Then the full suite. **Run `scripts/run_lint_gates.sh` — all of it.**
Hand-picking gates is how three breaks reached main in one day. It is generated
from `test.yml` and currently carries 64 steps.

```bash
./scripts/run_lint_gates.sh
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime
RUST_TEST_THREADS=1 cargo test --release -p perry-stdlib
# plus each integration suite the train touches:
RUST_TEST_THREADS=1 cargo test --release -p perry --test <suite> --no-fail-fast
```

- `perry-runtime` and `perry-stdlib` are **not parallel-safe** — they share
  process-global side tables. `RUST_TEST_THREADS=1` always. A failure whose
  message is about a *fixture precondition* ("must start proven") is another
  thread wiping a global, not a defect in the code under test.
- The runtime/stdlib `.a` come from the **`-static` wrapper crates**. Building
  `-p perry-runtime -p perry-stdlib` does not emit them, so you link a stale
  archive and **both arms of an A/B behave identically** — a vacuous
  "no regressions". Integration tests under `crates/*/tests/` need the wrappers
  built first for the same reason.
- **Set `PERRY_WORKSPACE_ROOT` for `crates/perry/tests/*` integration suites**
  when `CARGO_TARGET_DIR` lives outside the worktree. Otherwise the compiled
  `perry` cannot find the workspace, silently falls back to the *prebuilt*
  stdlib, and every test in the suite dies on the coherence stamp
  ("runtime library does not match this Perry compiler"). All-tests-fail at the
  same helper line is the signature — read the stderr before believing it is a
  regression. It also flips `ws` to `perry-ext-ws`.
- Check the **harness's** exit code, not a wrapper shell's. A pipeline through
  `grep` reports 0 while the harness failed. zsh does not word-split `$c`.
- `--profile perry-dev` inherits release, so `debug_assert!` is compiled out —
  a SIGABRTing runtime suite reads green. Don't validate with it.

Run one chain at a time. Two concurrent chains contend on the target-dir lock
and each blocks the other.

## 6. Landing

```bash
git push -u origin train<N>
gh pr create --title "..." --body-file <file>     # never inline --body
gh pr merge <train-pr> --rebase --admin
```

Then, always:

```bash
git fetch origin main
git diff origin/main train<N> --stat              # MUST be empty
```

Close each fork original with a comment pointing at the train. Use one
`Closes #N` keyword per issue — GitHub evaluates them at merge and one keyword
closes exactly one issue. Verify with `gh issue view <n> --json state`.

Post-merge traps:

- **Rebase-merge mints new SHAs.** A binary built from the pre-merge train
  branch fails the runtime coherence stamp against merged main even though the
  trees are identical — the stamp compares commits, not content. Any post-merge
  sweep needs a binary built at the merged commit.
- **Never `cp` over an existing signed macOS binary.** Same inode + new content
  = stale CDHash cache = instant SIGKILL, empty logs, every test reporting a
  compile error with an empty `.compile_error.log`. `rm` first, then `cp`.
- **Rebuild the compiler after every commit**, including docs-only ones, or
  ext-routed gap tests fail on commit skew rather than on your change.

## 7. Reporting

Say what was verified and how. "Gates green" means you ran them; "tests pass"
names the suites. If something is pre-existing on main, prove it by A/B against
the merge base before calling it pre-existing — a regression list is a
hypothesis until then. "Main is green" is worthless evidence unless main's run
descends from the suspect commit.

Report what you held and why, in one line each. Do not chase CI to green:
one pass classifying failures as mine-or-pre-existing, say so, stop.

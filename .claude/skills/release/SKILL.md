---
name: release
description: Create a new Perry release via the tag-LAST pipeline — dispatch release-packages.yml in cut_release mode on a pinned branch, let it gate + build everything, and only then does CI create the vX.Y.Z tag + GitHub Release and publish
disable-model-invocation: true
argument-hint: [optional free-text notes for the operator; the release notes themselves come from changelog.d fragments]
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

# New Perry Release (tag-last)

## Model (important — read before acting)

Perry releases are **tag-last**: nothing public happens until the test gate and every build leg are green. You dispatch `release-packages.yml` with `cut_release=true` on a branch pinned at the release candidate; the workflow then:

1. `preflight` — resolves `vX.Y.Z` from `Cargo.toml` on that SHA, fails fast if the tag already exists, if CLAUDE.md's `**Current Version:**` disagrees, or if `changelog.d/` has no fragments.
2. `await-tests` — dispatches `test.yml` + `simctl-tests.yml` on the branch if no run exists on the SHA yet, then polls by head SHA until both are green.
3. `build` + `build-cross` — all release binaries, archives as workflow artifacts.
4. `create-release` — **only now** creates the tag + GitHub Release (notes concatenated from `changelog.d/` fragments via `cut_release_notes.sh --notes-only`), and dispatches the tag-rider workflows (docs, benchmark, container-tests) on the new tag.
5. `publish-assets` → homebrew / apt / apt-repo / winget / npm / update-workers.

So a red gate or a broken build leg costs **nothing**: no burned tag, no half-published release. Version numbers only get consumed by releases that actually shipped.

`/release` does **not** bump versions, commit code, or write release notes — versions ride on every merged PR, and notes are the accumulated `changelog.d/` fragments.

## Steps

### 1. Sanity checks

- `git status` — **must be clean**. If not, STOP and report.
- `git rev-parse --abbrev-ref HEAD` — must be `main`. If not, STOP.
- `git fetch origin && git log HEAD..origin/main --oneline` — must be empty. If origin is ahead, pull/resolve first.
- `ls changelog.d/[0-9]*.md` — at least one fragment must exist (they become the release notes). None → nothing to release, STOP.

### 2. Read the version + verify the tag is free

- `VERSION` from `[workspace.package]` in `Cargo.toml`; must match CLAUDE.md's `**Current Version:**` line. Disagreement → STOP (preflight would also catch it, but catch it here in seconds).
- `git ls-remote origin "refs/tags/v$VERSION"` — must be empty. A hit means this version already shipped; land a bump first.

### 3. Pin the candidate + dispatch

```bash
git branch "release/v$VERSION" HEAD
git push origin "release/v$VERSION"
gh workflow run release-packages.yml --ref "release/v$VERSION" -f cut_release=true
```

The pinned branch matters for two reasons: `workflow_dispatch` always runs on a ref's **tip** (so `main` moving would shift the SHA under you), and `test.yml`'s `test-<ref>` concurrency group means dispatching tests on `main` cancels a running nightly.

Optional pre-warm: dispatch `test.yml` + `simctl-tests.yml` on the branch yourself right away — the gate matches any run on the SHA, so pre-flighted runs subtract their ~30 min from the critical path. If you skip this, `await-tests` dispatches them for you.

### 4. Watch

```bash
gh run watch $(gh run list --workflow="Release Packages" --limit 1 --json databaseId --jq '.[0].databaseId')
```

Expected timeline: gate ~30 min (queue-dependent; 120-min budget) → builds ~30-40 min → create-release seconds → publish ~20 min.

### 5. After success: fold fragments + clean up

The release exists, but the fragments that became its notes are still on `main`:

```bash
./scripts/cut_release_notes.sh --fold "v$VERSION"   # removes exactly the fragments recorded at the tag, commits
git push origin main                                 # or via PR, per protection rules
git push origin ":release/v$VERSION"                 # drop the pin branch
```

`--fold` is tag-scoped: fragments merged after the release SHA survive for the next release.

### 6. Verify (optional but recommended)

```bash
cargo install --path crates/perry --force && perry --version   # should print X.Y.Z
```

Report back: GitHub release URL, whether release-packages was green end-to-end, local `perry --version`.

## Failure modes

- **Preflight red** (tag exists / version drift / no fragments): nothing ran. Fix, re-dispatch.
- **Gate or build red**: nothing was tagged or published. Fix on `main` (normal PR, version bumps at merge), delete + re-pin the branch at the new candidate, re-dispatch. No version number is burned — the same `vX.Y.Z` can try again if the version didn't move.
- **Publish leg red (after create-release)**: the tag + release exist with partial assets. `gh run rerun <run-id> --failed` reruns the failed legs and everything downstream of them; the npm job is idempotent (skips already-published versions). Never delete/re-create the tag.
- **Branch moved under the dispatch**: `await-tests` fails fast with "ref moved" — re-pin and re-dispatch.

## Fallback: legacy tag-first path

Still fully supported if CI dispatch is unavailable: `./scripts/cut_release_notes.sh vX.Y.Z` from a clean main creates the tag + release (notes from fragments, removal committed for you); the `release: published` event runs the same pipeline with the same test gate. Re-publish an existing release with `gh workflow run release-packages.yml -f existing_tag=vX.Y.Z` (add `-f publish_npm=true` to redo npm).

## What NOT to do

- Do not create the tag or GitHub Release yourself in the tag-last flow — `create-release` does it, and a pre-existing tag makes preflight abort.
- Do not re-tag or force-push tags, ever. A failed publish is rerun-able; a mutated public tag is not.
- Do not commit anything during the release except the `--fold` commit afterwards.
- Do not use `git add -A` anywhere in this skill.

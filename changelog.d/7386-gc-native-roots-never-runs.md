### Fixed

**`gc-native-roots` had never completed a single run. It runs one arm on PRs now, all four on `main`.**

Measured 2026-08-04: **40 of 40 runs queued, zero completed**, the oldest sitting
nine hours. Every other workflow in the repo drained 2–3 runs per 20 in the same
window — `gc-ratchet` 2/20, `gc-root-dominance` 2/20, `test.yml` 3/20,
`gc-native-roots` **0/20**.

The cause is the matrix: four runner classes, two of them the scarcest GitHub
offers (`macos-14`, `windows-latest`), fanned out on every push to every branch.
With ~20 of its own runs competing for those runners it could not drain, and
cancelling the backlog refilled it within minutes because merges outpace the
drain rate.

A gate that never executes is the purest form of CLAUDE.md's fourth failure
mode — it cannot fail, because it cannot run. Every green PR that showed this
workflow "pending" was showing an assertion nobody had ever evaluated, including
every PR in the campaign that built it.

Pull requests now run the `ubuntu-latest` arm only; `main` and
`workflow_dispatch` keep all four. `ubuntu-latest` is the PR arm because its
runners are plentiful and it exercises the least-redundant path: x86-64 roots
resolve through the CFA-derived SP base (#7349), which neither the aarch64 x29
chain walk nor Windows' `RtlVirtualUnwind` shares.

The cost, stated rather than buried: a break confined to macOS, Windows or
aarch64-ELF now surfaces on `main` instead of on the PR. That is worth taking
while the alternative is surfacing nowhere. `gc-native-roots-complete` reads the
matrix job's aggregate result, so it stays a stable context for branch
protection whichever arm count ran — which is what makes promoting it to
required possible at all.

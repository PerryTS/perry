### Documentation

- **The engine plan's headline numbers were stale, and two changelog fragments
  collided with real PR numbers (#7572).** Documentation and metadata only.

  `docs/engine-plan.md` still listed the two worst rows of the v0.5.1299 sweep at
  their pre-fix values, both of which have since moved by an order of magnitude:
  `object_deep_clone` from 657.0 ms / 37.5× bun to **40 ms** (~2.3× bun, and
  **0.67× node — a win**) via #7540, and `map_1m` from 1233.7 ms / 4.8× bun to
  **309.1 ms** (1.40× bun, **0.96× node — a win**) via #7561. The
  `json_polyglot` `field_access` inversion — the optimized configuration running
  2.2× *slower* than the unoptimized one — is closed by #7537 and #7539:
  2984 ms σ 136 / 219 MB becomes **1809 ms σ 17.3 / 155 MB**, an 8.3× collapse in
  σ, and turning the tape on is no longer worse than turning it off.

  **A stale plan is not cosmetic — it is how effort gets spent on a
  non-problem.** Three times this campaign a ticket was picked up and profiled
  from a headline number that had already collapsed: #7510 (33.6% → 11%),
  `layout_forget_object` (14.5% → 3.0% → 1.7%), and `layout_note_slot`
  (7.5% → 0.03%, correctly closed with **no code at all**). The re-synced
  section now carries that instruction in place: re-measure the row before
  profiling it.

  The v0.5.1299 table is **kept intact rather than overwritten**. It was a single
  measurement event at 11 runs per cell; the fixes above were each measured
  individually, on the same pinned quiet mini, as part of the change that made
  them. Splicing individually measured rows back into a sweep table would claim a
  coherence the numbers do not have, so fixed rows are annotated in place and the
  deltas live in their own section attributed to the PR that moved them. The
  artifact still owes a fresh sweep, which stays blocked on #7475.

  Also records #7566's landing (workstream A of the #7469 construction campaign:
  the inline bump allocator at `new` sites inside loops, 1.81× on `churn_alloc`
  at +0 bytes for non-loop sites, and the honest 1.4% `tree` cost), together with
  the finding that the measurement justifying the previous default had *inverted*
  without anyone re-checking it — and the Mach-O negative result, that there is
  no local-exec TLS model and `-Ztls-model=local-exec` leaves the `blr` through
  the TLV descriptor byte-identical.

  Separately, two fragments were named with an issue number rather than their PR
  number, landing on numbers belonging to different changes:
  `7565-iterator-no-silent-truncation.md` → **7567** (#7565 is the
  `_tlv_get_addr` change) and `7566-macos-gap-snapshot-fallback.md` → **7568**
  (#7566 is the inline bump allocator). Left alone, `cut_release_notes.sh` folds
  them under the wrong PR, and two distinct changes sharing a number make the
  notes unreadable at exactly the point nobody can still check them.

  The rule is worth stating because the failure is systematic, not careless: a
  fragment written *before* its PR exists has to guess the number. Open the PR
  first and name the fragment from the number it is assigned — which is how this
  entry was written.

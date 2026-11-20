- **The release body is capped, and a tag left behind by a failed attempt no
  longer wedges the job.** `create-release` failed v0.5.1519 with
  `HTTP 422: body is too long (maximum is 125000 characters)`. The notes are
  generated from `changelog.d/`, and this release carries **1505 fragments** —
  the first tag since v0.5.1220 on 2026-07-04 — producing **2.8 MB** of notes,
  22× GitHub's limit.

  Two things made that worse than a simple failure. `gh release create` creates
  the tag and *then* POSTs the release, so the 422 left `v0.5.1519` pointing at
  the right commit with no release attached. And the guard above it aborted
  unconditionally on any existing tag, so every retry then died on the debris of
  the first attempt — the job could not succeed again by any path.

  Now: the body is truncated on a line boundary to 120,000 characters with a
  pointer to `changelog.d/` at the tag, and an existing tag is reused when it
  points at **exactly** the candidate SHA (still a hard error when it points
  anywhere else, which is the case the guard was written for).

  This would have blocked every release with a large fragment backlog, not just
  this one. Verified against the real 2.8 MB notes: the result is 120,221 bytes
  and ends cleanly.

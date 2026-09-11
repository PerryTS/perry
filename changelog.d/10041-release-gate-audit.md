- Harden the release-pipeline fixes during merge audit. Ancestor reuse rejects
  comparisons at the API's 300-file completeness boundary, non-ancestor results,
  unsafe paths, and renames whose old path is not release plumbing. Simulator
  tests always dispatch and poll the candidate, even when the main full-suite
  gate is reused from an ancestor.
- Read tag state once, accept only a valid 40-character commit SHA, and treat
  only HTTP 404 as absence; API failures cannot fall through to publication.
- Bound each npm visibility request, including response-body reads, by the
  remaining shared deadline. Poll unresolved packages round-robin and require
  both publication time and the expected immutable hash before releasing the
  wrapper. Failure-path tests run in lint, including an actually stalled local
  HTTP response and proof that failed visibility withholds wrapper publication.
- Preserve missing-tarball diagnostics with an empty array on Bash 3.2, and
  correct the historical description of the LLVM apt action's update handling.

- **Version bumped to 0.5.1520 after npm left v0.5.1519 half-published.** npm
  staged `@perryts/perry-linux-x64@0.5.1519` and never finalised it, leaving the
  version invisible (404, absent from the packument's `time` map) *and*
  un-republishable (`E409 — Cannot publish over previously staged version`).
  Six of the seven packages went public, including the wrapper, so
  `@perryts/perry@0.5.1519` shipped as `latest` with a platform dependency that
  does not resolve on linux-x64.

  0.5.1519 cannot be completed from our side while that staged version persists,
  so the release moves to 0.5.1520. Nothing about the build was wrong: all 14
  legs were green and the packed tarballs are reproducible — a rerun skipped
  every already-public package on a *matching* sha1.

  Worth recording, because it cost about 45 minutes of looking in the wrong
  place: `npm publish` printed `+ @perryts/perry-linux-x64@0.5.1519` and "your
  package is being processed", and signed provenance into sigstore, all while
  npm held no record of the version. The check that distinguishes "processing"
  from "never landed" is the packument's `time` map, not `npm view` — a
  published version appears there immediately.

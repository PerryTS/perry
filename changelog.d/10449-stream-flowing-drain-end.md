### Fixed

- `PassThrough` and `Transform` streams no longer replay chunks or emit `end`
  merely because their readable buffer drained. Flowing chunks are consumed when
  emitted, while the readable side stays open until EOF or an explicitly finite
  source is exhausted (#10449, PR #11041).

### Fixed

- **Regex separators that require fancy-regex now run the same
  `RegExp.prototype[Symbol.split]` cursor algorithm as ordinary patterns.** The
  old `find_iter` fallback emitted a trailing `""` after a zero-width match at
  the end and discarded separator captures. The fancy lane now uses
  `captures_from_pos` on the complete subject, performs the spec's sticky
  `q`/`p` walk bounded by `q < size`, splices matched and unmatched captures,
  and stops as soon as `limit` is reached.

  Coverage includes lookbehind and lookahead separators, captures, start/end
  matches, empty subjects, limits, and the multiline `^` / `$` forms rewritten
  onto the fancy lane by #9427. The pre-existing runtime test that pinned the
  incorrect trailing element now asserts Node's result.

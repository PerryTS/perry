### Performance

- **`[^]` and `[]` no longer make regex construction case-fold a million code
  points.** Perry translated JS's "any character" class to `[\s\S]` and its
  empty class to `[^\s\S]`. Under the `i` flag, `regex_syntax`'s
  `case_fold_simple` walks *every code point in a range*, and `[\s\S]`
  canonicalizes to `\x{0}-\x{10FFFF}` — so each such class ran a 1,114,112-step
  loop to compute a fold that cannot change anything. They now translate to
  `(?s:.)` and `[a&&b]`, which are never folded. Isolated: `(?i)[\s\S]*?`
  7.72 ms → 41.7 µs (185×), `(?i)[^\s\S]` 7.46 ms → 11.4 µs (654×). On
  claude-code's `--help`, −336 million instructions (**−3.78%**), with output
  byte-identical to node.

### Bug Fixes

- **A lookbehind or backreference literal could stop matching for good.**
  The three compiled-program caches are capped independently and each
  `clear()`s wholesale on overflow, and `compile_and_cache_regex_checked`
  returns early whenever `REGEX_CACHE` already holds the pattern — so it never
  re-runs the fancy-regex or repeat-matcher build. For a pattern only
  `fancy-regex` accepts, the `REGEX_CACHE` entry is the never-match
  placeholder and the real program is the one in `FANCY_CACHE`; once
  `FANCY_CACHE` reached its 512-entry cap and cleared while that placeholder
  survived, `get_or_compile_regex` handed back a program matching nothing and
  nothing rebuilt the fallback.

  Since `lookup_fancy_regex` treats a built header as authoritative (a null
  `fancy_ptr` beside a non-null `regex_ptr` IS the answer) and
  `site_cache::install_programs` memoizes that triple against the pattern
  text, this is not one bad header: every later construction of the same
  literal is born with it, until the site-cache entry is evicted. The same
  shape applies to `REPEAT_MATCHER_CACHE`, where the wrong answer is quieter —
  the linear engine's capture assignment instead of ECMA-262's RepeatMatcher
  semantics.

  `lazy::build_and_install_programs` now repairs a missing program before
  publishing the header and before memoizing the triple: if the standard
  program is the never-match placeholder and no fancy program came back, it
  rebuilds the fancy one; if no repeat matcher came back, it re-derives it
  (`repeat_matcher::compile` is a byte scan that returns immediately unless a
  capture group sits under a quantifier, so it is free for the patterns that
  do not need it). A built header therefore always carries every program its
  pattern needs, which is exactly the invariant the header-authoritative
  lookups and the construction cache depend on.

  (`a_single_program_cache_clear_cannot_disarm_a_lookbehind_literal`)

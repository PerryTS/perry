### Performance

- **RegExp construction and exec no longer hash or copy the pattern text.**
  On the claude-code TUI a keystroke re-runs ink's layout, whose text
  measurement (`string-width` / `emoji-regex` / `ansi-regex`) evaluates a
  regex literal per text segment — `emojiRegex()` is a fresh ~12 KB `/…/g`
  per call. Each `js_regexp_new` copied that pattern three times and
  SipHashed it once (the `VALIDATED_PATTERNS` probe key, `owned_pattern`,
  the `REGEX_SOURCE_TABLE` entry); the first operation on each header did
  the same three more times in `build_and_install_programs`; and, for the
  common pattern with no fancy fallback, `lookup_fancy_regex` and
  `lookup_repeat_matcher` fell through to a full clone + hash of the pattern
  on EVERY exec. SipHash over pattern text was 31 % of the main thread in
  the 20 s after a 400-char reply had rendered (regex 38 % inclusive).

  - `crates/perry-runtime/src/regex/site_cache.rs` (new) — a thread-local,
    content-keyed construction cache: a cheap fingerprint (length, three
    8-byte windows, canonical flags) plus a full byte compare, so identity
    never depends on an address. A hit skips validation (validity is a pure
    function of the pair), shares the owned pattern/flags as `Arc<str>`, and
    installs the programs the first executed header compiled — the new
    header is born built and never touches the `(pattern, flags)` caches.
    Kill switch `PERRY_REGEX_SITE_CACHE=0`.
  - `regex.rs` — `lookup_fancy_regex` / `lookup_repeat_matcher` treat a
    built header as authoritative (a null program pointer after the build
    IS the answer; every install path publishes all three together), so no
    per-exec cache probe remains. `REGEX_SOURCE_TABLE` holds `Arc<str>`
    pairs; the two address-keyed regex tables use the pointer hasher.
  - `regex/exec.rs` — `test` on a global/sticky receiver runs
    `regexp_find_advancing`, the find-only twin of `exec`'s engine phase
    (same engine order, `lastIndex` advance/reset and sticky anchoring),
    instead of materializing a captures array plus one string per capture
    that it then discarded.
  - `hot_diag.rs` (new) — `PERRY_REGEX_DIAG=<path>` (constructions,
    validated/site hits, pattern bytes, compiles, cache clears, lazy builds,
    exec/test/match/replace counts, capture bytes, per-pattern table) and
    `PERRY_IC_DIAG=<path>` (property-read IC misses by reason and by site).
    Snapshots every ~1 s of activity; diagnostic only.

  Tests: `site_cache_reconstruction_is_born_built` (fails without the
  cache) and `global_test_advances_and_resets_last_index` (every
  `lastIndex` branch of the find-only path, all three engines, UTF-16
  units) in `regex/tests.rs`.

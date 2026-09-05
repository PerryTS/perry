### Performance

- **Probing the compiled-program caches no longer materialises the key.** The
  three thread-local caches were `HashMap<(String, String), _>`, and
  `HashMap::get` needs a `&(String, String)` — so **every probe allocated two
  Strings and copied the pattern text into them**, on a path that runs once per
  RegExp OBJECT, and a JS regex literal evaluates to a fresh object every time
  it is reached. A native-churn census of the claude-code binary (2026-09-05)
  put `js_regexp_test` → `lookup_repeat_matcher` → `build_and_install_programs`
  at **6,044 MB of 8,334 MB of estimated allocation with zero live bytes** —
  73 % of all remaining native churn — split across the three probe sites: the
  `get_or_compile_regex` probe (2,071 MB) and two `core::fmt::Formatter::pad`
  frames (1,989 MB and 1,984 MB), which is what `.to_string()` on an `Arc<str>`
  lowers to.

  The caches are now keyed by `ProgramKey = (Arc<str>, Arc<str>)`. Every caller
  that matters already holds those `Arc`s — `REGEX_SOURCE_TABLE` and
  `regex::site_cache` share one allocation of a literal's text with every
  header built from it — so a probe is two refcount increments and no
  allocation at all. The two remaining `Arc::from` materialisations are on cold
  paths: the syntax-error fallback in `js_regexp_new` (a pattern the linear
  engine's parser refused, 7.7 % of real literals, once each) and
  `RegExp.prototype.compile` (once per call from user code).

  Hashing still walks the pattern bytes; the allocation is what the census
  measured and what this removes.

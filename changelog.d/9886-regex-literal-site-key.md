**A regex literal is now identified by its SOURCE SITE, not by its text**, so
constructing one costs a single word compare instead of a content fingerprint
plus a full byte compare of the pattern.

A regex literal evaluates to a fresh object every time it is reached
(ECMA-262), and TUI code reaches them inside hot functions: `string-width`'s
`emojiRegex()` returns a fresh ~12,807-character `/…/g` on every call, once per
grapheme in claude-code's layout pass. The runtime therefore re-derived "which
pattern is this?" from the text on every construction — `regex::site_cache`
keys on a cheap fingerprint and, because a fingerprint can collide, verifies
every hit with `&*entry.pattern == pattern`. That verify is linear in the
pattern: `PERRY_REGEX_DIAG` measured **2.0 GB of `memcmp` per 400-character
reply**, and a `sample` of the segment loop put `_platform_memcmp` at **39.6 %
of `js_regexp_new`'s own subtree**.

The compiler knew the answer all along; the lowering just had no way to say it.
`Expr::RegExp` now emits an 8-byte private global per literal site and passes
its **address** as a third argument to a new `js_regexp_new_site(pattern,
flags, site_key)`. That address is unique by construction, immortal, and never
moves — which is exactly what a `StringHeader` address is not, and why the
earlier analysis of this problem concluded no sound string identity existed and
left the byte compare in place: string headers are GC-managed, so an address is
freed and reused, and a moving collector relocates them.

A hit verifies with one word plus the site's ≤ 8-byte flags text (two spellings
of one canonical form must not answer for each other) and then reads nothing
about the pattern at all: no fingerprint, no `memcmp`, no validation — validity
is a pure function of `(pattern, flags)` and the site's first construction
established it — and no flag canonicalization, since the seven flag bits are a
property of the site. Once the site's first header has executed, later
constructions are born built.

`site_key = 0` means "no site" and behaves exactly as before, so every dynamic
construction (`new RegExp(s)`, `js_regexp_construct`,
`RegExp.prototype.compile`, the runtime's own callers) keeps the two-argument
entry point and never touches the site table — pinned by a test that asserts
the table is still empty after four dynamic constructions, and non-empty after
one site-keyed one, so the zero is a property of the entry point rather than of
a table that never works.

The named sabotage is a table keyed by anything weaker than the site address:
two literals at two sites, same flags, **same pattern length**, different text.
Under a length- or prefix-keyed table the second site inherits the first's
entry, `.source` reports a pattern the literal never contained and `test`
matches the wrong language. Each site is constructed twice, because a first
construction always misses and would pass under every sabotage.

Kill switch: `PERRY_REGEX_SITE_KEY=0` — the probe misses and nothing is
recorded, so the OFF arm is the content-keyed path exactly rather than a
control still paying the bookkeeping.

The new runtime symbol is declared in `runtime_decls/strings.rs` with a test
asserting its **name and arity**: a missing `declare` is invisible to every
HIR-level test and fails only at the in-process LLVM parse (`use of undefined
value`), and a wrong arity parses and miscompiles.

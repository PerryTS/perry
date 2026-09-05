### Runtime

- perf(string): a one-ASCII-character string is now the canonical per-thread
  header instead of a fresh 32-byte allocation. `js_string_char_at` — and
  everything that funnels through it (`s[i]`, `charAt`, string spread, the
  String-wrapper index installer) — used to mint one string per character read,
  which on a text-measuring workload is the single largest source of garbage.
  Same residency contract as the existing small-integer string table
  (longlived arena, `refcount = 0`, pinned, scanned by the same root scanner —
  no new scanner is registered).

- perf(runtime): `String`/`Number`/`Boolean`/`BigInt` wrapper dispatch,
  `x.constructor`, `toString` resolution and the `globalThis` builtin lookup
  resolve their constant property names through the intern table instead of
  minting a heap string per lookup. `js_get_global_this_builtin_value` alone
  allocated 133 MB during a 3300-character claude-code reply, all of it the
  same handful of literals. Interned keys also make the property-read and
  property-write fast paths eligible, which a freshly minted key never was.

- perf(runtime): a `String` wrapper no longer stores one property descriptor
  per character. ECMA-262 §10.4.3 gives every in-range index of a String
  exotic object `{ writable: false, enumerable: true, configurable: false }` —
  a fact of the class and the boxed length, not per-object state — so
  `get_property_attrs` answers it from the wrapper's payload. Storing it cost,
  per boxed character, a Rust `String`, a hash-map entry only a full
  collection could reclaim, an owner-index entry, and one program-wide
  `prop_plan_epoch_bump()`. A sloppy method call on a string primitive boxes
  its receiver, so the compiled claude-code TUI paid that for every rendered
  line.

**The six `replace`/`replaceAll` entry points root their raw receiver across the argument coercion** (#6949 shape a).

`js_string_coerce` used as a plain **ToString argument** coercion allocates for every shape except an already-heap `STRING_TAG` value (`builtins::string_coerce_is_inert`): an SSO short string materialises onto the heap, a number/bool/null/BigInt builds its stringification, and a `POINTER_TAG` object runs a user `toString`/`valueOf`. Any of those can collect and evacuate. Rust evaluates arguments left to right, so the raw `*const StringHeader` receiver is copied *before* the coercion runs — the copy survives, the pointee moves, and the callee dereferences the stale one.

Fixed at all six sites in `regex/replace_fn.rs`, using the same `RuntimeHandleScope` idiom #6943 established for the property-key half of this family:

| function | rooted |
|---|---|
| `js_string_replace_string_dyn` | `s`, `pattern` |
| `js_string_replace_all_string_dyn` | `s`, `pattern` |
| `js_string_replace_search_dyn` | `s` |
| `js_string_replace_all_search_dyn` | `s` |
| `js_string_replace_regex_dyn` | `s`, `re` |
| `js_string_replace_all_regex_dyn` | `s`, `re` |

The two regex entry points matter beyond byte-reading: `re` is a `RegExpHeader`, so a stale one is consulted for its compiled pattern, not merely for characters.

**Stated plainly: I could not produce a failing witness.** A 200k-iteration fixture driving all six entry points with non-string replacements (so the coercion always allocates) produces node-identical output on both arms, including under `PERRY_GC_ZEAL=1` with `PERRY_GC_PROTECT_FROMSPACE=1` at depth 200 — and that run is not vacuous: the quarantine retired **27 page-sets and protected 179 MB**, so evacuation genuinely happened. The unfixed baseline survives it 3/3 as well.

That is characteristic of this class rather than evidence against it — the window needs the pointee to move *during* that specific coercion, and #7154's family is documented as invisible to every runtime probe at the moment of collection. The justification here is structural and identical to the one #6943 shipped on: a raw heap pointer held across a call that can allocate is a defect by the repo's own rooting invariant, whether or not today's allocator layout happens to expose it.

Scope is deliberately shape (a) of the three the issue enumerates. Shape (b) (constructors holding a fresh `obj` across a later coercion, in `messaging.rs`, `disposable.rs`, `boxed_primitives.rs`, `construct.rs`) and the third shape (raw `JSValue`s parked in Rust `Vec`s across allocations, in `groupby.rs` / `define_properties.rs`) are untouched — the third in particular is a different mechanism, since no GC scanner can see a `Vec<f64>` at all, and the issue itself flags it as needing its own decision.

Verified: `cargo test -p perry-runtime --lib` 2051 passed / 0 failed; `test_gap_string` 5/5, `test_gap_regex` 3/3; fmt, file-size and the addr-class audit all clean.

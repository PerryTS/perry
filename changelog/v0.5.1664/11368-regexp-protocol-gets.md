### Performance

- **`str.replace/replaceAll/match/split(regexp)` and `re.flags` no longer run their RegExp protocol Gets through the generic property path on an untouched RegExp** (#10518). Each of these reads `rx.flags` (whose getter makes eight more Gets), looks up `@@replace`/`@@match`/`@@split`, and for `split` also reads `rx.constructor[@@species]` and builds a new sticky splitter RegExp. Every one of those went through `js_reflect_get` inside a setjmp trap frame, and on a short subject they cost far more than the match itself.

  `object/regex_canonical.rs` already proved, per `RegExp.prototype` ShapeId, that an instance with no own properties and the untouched prototype reaches the builtin `exec` and flag accessors. That proof now also covers the three symbol methods (`@@replace`, `@@match`, `@@split` are the builtin data properties, keyed on the symbol-property epoch like `@@replace` already was) and the species constructor (`RegExp.prototype.constructor` is a data property holding the intrinsic `RegExp`, whose own `@@species` is still the builtin accessor). When it holds:

  - `String.prototype.replace/replaceAll/match/split` call the builtin symbol method directly instead of looking it up; `replaceAll`'s IsRegExp and `flags` checks read the header.
  - The `flags` getter returns the RegExp's stored canonical flags string (already in spec `dgimsuvy` order) instead of making eight Gets and assembling a new string.
  - `RegExp.prototype[@@split]` skips SpeciesConstructor, `Get(flags)` and the splitter construction. It runs the forward search (#10165) with the receiver's own program (or a `y`-less compile for a sticky receiver), bound through the validation witness (#10166) rather than re-validated per call. The general path's forward search now binds through the witness too.
  - `RegExp.prototype[@@replace]` allocates its results list only on the general path; the direct path never used it.

  Anything observable keeps the full protocol: a subclass, an own `flags`/`exec`/symbol property, a patched flag getter, `exec`, symbol method, `constructor` or `RegExp[Symbol.species]`, and a `limit` whose `valueOf` could change any of them mid-call.

  The issue's microbenchmark (N = 200,000, a 3-character subject, median of 5, Linux x64, `PERRY_NO_AUTO_OPTIMIZE=1`; before and after are the same commit built with and without the change):

  | variant | before ms | after ms | Node 22 ms |
  |---|---:|---:|---:|
  | `re.flags` | 1,586 | 172 | 3.9 |
  | `s.replace(/\+/g, " ")` | 736 | 479 | 16.2 |
  | `s.match(/\+/g)` | 482 | 268 | 16.0 |
  | `s.match(/\+/)` | 418 | 217 | 7.4 |
  | `s.split(/\+/)` | 3,266 | 374 | 17.1 |
  | jws base64url chain | 2,461 | 1,864 | 62.1 |
  | `s.replace(/\+/g, "")` (control) | 346 | 346 | 13.8 |
  | `/\+/.test(s)` (control) | 121 | 120 | 3.6 |

  Checksums identical. The rest of the gap to Node is the per-call engine cost tracked in #10166.

  Covered by `test-files/test_gap_10518_regexp_protocol_gets.ts`, which checks the fast answers and that each override above is still observed.

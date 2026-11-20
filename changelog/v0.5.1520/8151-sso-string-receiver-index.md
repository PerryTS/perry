### Fixed

- **Indexing a short concatenated string returned `undefined`** (#8117, reopening
  #6887 at a different funnel). `const s = "ab" + "c"; s[0]` read `undefined`
  instead of `"a"`, while `s.length`, `typeof`, printing, `charAt`, `for-of`,
  spread, `Array.from` and `split` were all correct. Every ASCII concatenation
  of five bytes or fewer is an inline `SHORT_STRING_TAG` (SSO) value, so
  `(a + b)[0]`, an index-accumulation loop, and `parts.join("") + "\n"` were all
  affected; the byte-identical heap string — a non-ASCII or >5-byte
  concatenation — was fine.

  `js_object_get_index_polymorphic` opened by asking *"does this receiver's low
  48 bits hold a heap pointer?"* and returned `undefined` for every tag that
  does not, conflating *not a pointer* with *not indexable*. It is the recurring
  family — a receiver-specific arm claiming the operation before the
  receiver-kind question is asked; this dispatcher asks the *key*'s kind
  carefully, including a dedicated SSO arm for an SSO key, and never asked the
  receiver's. #6888 had fixed codegen's proven-`string` fast path, but a
  receiver whose string type is not *proven* (an inferred `const`, an annotated
  `const s: string`, or a `string` parameter) reaches the generic dispatcher
  instead, which passes the raw NaN-boxed bits to this helper. The symptom
  changed from #6887's segfault to a silent wrong value because
  `is_valid_string_ptr` now rejects the bogus pointer rather than dereferencing
  it.

  Fixed by adding the `SHORT_STRING_TAG` arm, delegating to
  `js_string_index_get_boxed` so both string representations share one copy of
  the CanonicalNumericIndexString semantics. Fixing at the shared funnel covers
  every call site that funnels an unboxed receiver here. Sabotage-verified:
  removing the arm fails `an_sso_string_receiver_reads_its_characters` with
  `left: None  right: Some("a")` — the gap test's own wrong answer — while
  controls for the heap-string arm, non-string primitives, and
  out-of-range/fractional/`NaN` indices stay green.

  Fixes `test-files/test_gap_sso_concat_string_index.ts`.

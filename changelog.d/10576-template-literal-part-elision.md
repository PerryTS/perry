### Performance

- Cut the instruction count of a template literal that opens on a
  substitution (`` `${x}...` ``, the common shape — no literal text before
  the first `${`) by eliding two sources of wasted work in its desugared
  `js_string_concat_chain` call:
  - The leading quasi is skipped when empty instead of unconditionally
    seeding the chain with a literal `Expr::String("")`. Every *interior*
    quasi already had this guard; the leading one never did, so a template
    opening on `${` always carried one extra, always-empty part through
    classification, and a single-substitution template (`` `${x}` ``) missed
    the concat-chain fold's 3-part minimum entirely, falling back to the
    pairwise path to concatenate an empty string for nothing.
  - A `number`-typed parameter's substitution now drops its redundant
    `StringCoerce` wrapper even when codegen has no dataflow *proof* it is
    numeric, only the declared annotation — mirroring how
    `is_declared_string_expr` already trusts a declared `string` a few call
    sites up the stack. This is sound because `js_string_concat_chain`'s own
    part classifier tag-dispatches every part itself, and for any shape that
    isn't a plain number it falls back to the exact `js_jsvalue_to_string` /
    `js_string_materialize_to_heap` calls `js_string_coerce` forwards to for
    those same shapes — so a lying `number` annotation still produces
    byte-identical output, and only a genuine number additionally skips a
    throwaway intermediate heap string.

  Measured on `` `${s}:${n}` `` (`s` a short string, `n` a non-integer
  double), differencing two probes to cancel fixed per-process cost (median
  of 7, N=20000; `loop16`/`loop80` control read ~0 in both arms): **1173 →
  955 instructions per evaluation (−18.6%)**. An integer-interpolation
  variant (`` `${s}:${i}` ``, `i` a proven loop counter that already had the
  numeric fast path) isolates the leading-quasi fix alone: 713 → 675
  (−5.4%), confirming the larger non-integer win comes from the
  `StringCoerce` elision.

**`gc_root_dominance_check.py` could not see macro-defined runtime exports, so
two live `POLL_CAPABLE_RUNTIME` entries read as stale.**

`runtime_symbols()` matched `extern\s+"C(?:-unwind)?"\s+fn\s+(js_\w+)` — a
LITERAL name after `fn`. A symbol defined through a macro reads
`pub extern "C" fn $name` inside the macro body, so it was invisible. **56
exported `js_*` symbols were missing**, and `--audit-poll-capable` reported two
of them as naming nothing:

    js_string_replace_regex_fn
    js_string_replace_all_regex_fn

Both exist — declared by codegen at `runtime_decls/strings_part2.rs:404-405`,
defined by `regex_value!` at `regex/perex_replace_compat.rs:89-90`. They are
`String.prototype.replace(re, fn)`, which runs a **user JS callback**, so they
are unambiguous poll points. The obvious remedy the report invited — delete the
stale-looking entries — would have removed coverage of a real poll point and
turned the audit green, which is exactly what the audit's own error text warns
against. The scanner gave the reader no way to tell "stale" from "invisible".

This is the SECOND under-count in that one function. #8207 widened it for
`-unwind` and hid 18 symbols including `js_throw`. Same class, same direction:
silent, and always toward green.

**Fix.** `runtime_symbols()` now also collects `js_*` names passed to an
ITEM-POSITION macro invocation. Item position is the discriminator that works:
a macro defining an export sits at column 0, while `assert_eq!(js_thread, ..)`
inside a function body is indented and defines nothing. Three invocation shapes
occur in the tree and all three are covered — name inline after `(`, name alone
on a later line after a doc comment followed by `=>`, and the single-argument
shim form.

The body extractor had the same blindness one layer down, and it mattered more
quietly: a macro-generated symbol has no per-symbol body in source, so
`--audit-poll-reach` saw it calling nothing and could never report it as
reaching a poll point. Each macro's `macro_rules!` body is now attributed to the
symbols it generates. Over-attribution is possible and deliberate — it can add
an edge a specific arm would not have, which makes that audit stricter, never
blinder.

**`--verify-symbols ARCHIVE...` is the new guard**, wired into
`gc-root-dominance.yml` right after the archives are built. It cross-checks the
scanner against `nm -gj` on the real archives. nm is a LOWER bound — an archive
built for one target omits the other targets' cfgs — so the assertion is
`nm <= scanner`, and a symbol the linker emitted that the scanner cannot see is
the error. Measured on a release build: nm defines 3814, the scanner sees 3932
(56 macro-generated); the only names beyond nm are the 17 `js_wasm_export_call_*`
shims, which are real and simply absent from a non-wasm build. Sabotage-tested
by restoring the old narrow scanner: it reports 39 invisible symbols including
both of the ones above, and exits 2.

One entry WAS genuinely stale and is deleted: `js_ratelimit_new_from_options`,
whose crate went with the npm-binding strip. No definition, no codegen
declaration, nothing in nm. That is the difference the scanner could not express
before, and `--verify-symbols` is how the next person tells the two apart.

**Why column-0 is safe rather than lucky.** Item position is a heuristic, and it
is allowed to be one because a miss is CAUGHT rather than silent. Two checks
enforce that, at different costs:

- `--verify-symbols ARCHIVE...` compares the scanner against `nm` on the real
  archives, in `gc-root-dominance.yml` where the archives already exist. It
  catches ANY shape the regex misses — but it needs a build, and that workflow
  is label-gated, so it speaks on scheduled `main` runs, after the fact.
- `--audit-macro-item-position` is the build-free half and runs in `lint`, which
  IS a required context. It enforces the heuristic's PRECONDITION instead of its
  result: a macro invocation naming a `js_*` symbol at an indent (an export
  macro wrapped in an inline `mod`, say) fails the PR with the remedy — move it
  to column 0, or teach `_macro_defined_symbols` the shape, or declare the macro
  non-defining. Sabotage-tested by planting exactly that: it names the file, the
  line and the symbol, and exits 2.

That split matters because of #8821's precedent, cited in `test.yml`: a
build-free audit that lives ONLY in the label-gated workflow "is skipped on every
PR and speaks only on scheduled `main` runs, after the fact". The archive
cross-check is the durable guarantee; the required per-PR check is what stops the
regression reaching `main` in the first place.

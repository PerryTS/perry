Fix static private fields by defining their values directly in the constructor's private shape entries and using checked private reads and writes. Public property strings cannot expose or overwrite those entries; freezing the constructor does not freeze its private fields. This restores the construction gate used by lru-cache's Stack.

The first bad commit is c346a014e72dd1aaa0276609d6e9220431092e6b (PR #12083, "RegExp S2: a RegExp instance is an ordinary shaped object"). Its namespace-aware attribute edits stopped converting a public property into a private entry, exposing the static-field initialization path's incorrect public-store-then-claim sequence.

Regression coverage includes the Stack pattern, private fields, methods and accessors, separate class-expression evaluations, subclass and instance rejection, private-brand checks, initialization order, reflection, public-key isolation, and writes after Object.freeze.

Validation: the regression fails on main and matches Node 26.5.1 with the fix. Release runtime and codegen suites pass with zero failures; 77 of 80 nearby static/private gap tests match Node, up from 65 on main, with no new failures. Formatting, native ABI and Node-pin checks pass. The GC-effects audit has existing table drift, with no stronger existing effects than the preserved main runtime and both new entries correctly classified as Reenters.

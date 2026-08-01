`--opt-report`'s `Ptr<Shape>` section stops charging Perry's own CommonJS
wrapper to the user's code. Over 195 real `__esModule` dependency modules from `scriptc/node_modules`,
**31 % of all `Ptr<Shape>` candidates (379 of 1231) were two statements the
`cjs_wrap` template emits itself** — and they were the evidence behind two
scheduling decisions that turned out to be about scaffolding rather than about
dependency code.

| denial bucket | base | after |
|---|---:|---:|
| rule 2 — bare reference | 140 | **4** |
| rule 1 — unbound alloc, constructor argument | 196 | **8** |
| rule 5 — module barrier | 187 | **132** |
| rule 1 — unbound alloc, all positions | 885 | **759** |
| total `ptr-shape` candidates | 1231 | **914** |
| selected / consumed | 3 / 11 | **3 / 11** |

Every removed denial is named: 136 of the 140 rule-2 bare references were the
local `__cjs_module`, as were 55 of the rule-5 denials, and 188 of the
constructor-argument allocations were the `{}` inside
`const __cjs_module = { exports: {} }` — 379 rows over 195 modules, almost
exactly two per module. Zero `__cjs_module` rows remain in any bucket. The net
drop is 317 rather than 379 because removing them **un-masks 62 real user
rows** (see below). Selections and consumptions are unchanged, which is the
point: nothing here promotes anything.

## The shape

`cjs_wrap`'s preamble emits, into every wrapped module:

```js
const __cjs_module = { exports: {} };
var module = __cjs_module;          // ← the rule-2 "bare reference"
…
require.main = module;
```

`__cjs_module` is a `Stmt::Let` of an object literal, so rule 1 seeds it as a
candidate; the next statement aliases it into a `var`, so rule 2 denies it.
The inner `{}` is an allocation in constructor-argument position, so rule 1
denies that too. Both fire once per CommonJS module, in every CommonJS module.

**The reference is not exemptable, and that is a finding, not a limitation.**
`module` is a reassignable `var` that the preamble goes on to store into
`require.main`, and that CommonJS bodies write `module.exports = X` through.
The record genuinely escapes; no narrowing of rule 2 could promote it. What is
wrong is that it was ever a *candidate* — promotion would buy nothing (a
one-field record, read three times at module-init, loop depth 0) and can never
be proven. So it is suppressed at the seed, not exempted at the rule.

## The recogniser (`collectors/cjs_scaffolding.rs`, extends #7139)

A region is a CommonJS preamble when its top level carries **R1** a
`mutable: false` `Let` named `__cjs_module` initialized by an `__AnonShape_…`
allocation, **R2** whose literal is exactly `{ exports: {} }`, **R3** uniquely
in the region, and **R4** aliased by a `mutable: true` `Let` named `module`
whose init is a bare `LocalGet` of it.

**R4 is the soundness argument, not a heuristic.** It *is* the denial: that
statement shape walks into `UseWalk`'s `LocalGet` arm under the default escape
context and disqualifies the record on every path, and `mutable: true` keeps
the alias pre-pass from tracking it. A region satisfying R4 cannot promote its
record, so removing it from the candidate set leaves the returned facts
bit-identical. R1-R3 only make the recognition unambiguous. Dropping a
candidate can only remove facts, never add one — the opposite direction from
#7139's barrier exemption, which relaxed a proof.

Recognised, the region also stops reporting the object literals of the four
preamble statements behind the record: the two `defineProperty` sites #7139
already recognises (same predicate, so the two exemptions cannot disagree about
what scaffolding is) plus `require.cache = {}` and `require.extensions = { … }`.
That half is report-only in the strongest sense — `unbound_new_sites` runs
only under `opt_report::enabled()`.

## Why the whole preamble and not just the record

`--opt-report` dedups allocation-site rows per function on
`(module, function, name, position, rule)`, and every object literal renders as
`object literal { ... }`. So each wrapped module contributes exactly **one**
alloc-site row, and which context it reports is whichever preamble literal the
walk reaches first. Suppressing only the record would have moved the row from
`constructor argument` to `statement` and changed the total by zero. The same
dedup is why `statement` rises 273 → 320 here: with the scaffolding row gone,
a *user* allocation that was being masked by it surfaces. Removing 317
scaffolding rows made 47 real ones visible.

## Evidence

- **Codegen-neutral, measured**: emitted LLVM IR compared base vs fix over 49
  dependency modules, each with a same-compiler control run — 47 identical, 0
  different, 2 excluded because their own control run differs (HashMap-ordered
  `perry_method_<Class>__<name>` emission when one method name is shared across
  classes; pre-existing, #7131 family). The one nondeterminism that IS
  normalised is HashMap-ordered `js_register_function_name` string constants,
  16 of ~12 900 lines; nothing else is. Object-file hashes are unusable here —
  the `--no-link` temp output path lands in the Mach-O debug records, so the
  same compiler run twice differs by ~2 KB.
- **Behaviour**: a CJS dependency fixture (`__esModule`, class, record,
  1000-iteration loop) compiled and run by both arms — byte-identical output,
  byte-exact against Node 26.5.1. Report on the same fixture: 5 candidates /
  2 denied → 3 candidates / 0 denied, with the *same three* selections.
- **Sabotage, 16 rows, each with a named red set**: every conjunct deleted or
  weakened in turn (R1 mutability / name / literal-ness, R2 arity / emptiness,
  R3 uniqueness, R4 presence / mutability / identity, the `require`-key
  whitelist, the candidate filter, the alloc-site skip) plus four template
  edits in `wrap.rs`. Control green in all 17 runs. The first pass had **four
  green holes**; all four were fixed in the code, not the tests — the binding
  name and the uniqueness count each had two enforcement points that masked one
  another, and the "is a record recognised" gate was a removable `if` that no
  test could kill. The record and its scaffolding bindings now live in one
  `Option<(u32, CjsScaffolding)>`, so "unrecognised but still suppressing" has
  no representation.
- **Template canary** (`cjs_wrap/preamble_canary_tests.rs`, extending #7139's):
  runs the real template through the real recogniser, with one anti-vacuity
  assertion per conjunct naming the conjunct it broke, and a negative control
  that a never-wrapped module recognises nothing.
- `cargo test -p perry-codegen --lib`: 472 passed (30 new). Census green, no
  floor moved.

## What this does not do

Nothing is promoted. Conversion to selected/consumed is **zero**, by
construction: every suppressed value was denied in the base arm too, which is
exactly what R4 asserts.

The dependency-JS wall stays where #7152 put it — rule 1, allocations never
bound to a local — but its size needs restating. In this corpus rule 1 goes
885 → 759: 188 base rows were the `{ exports: {} }` literal and 62 previously
masked user rows appeared. #7152's corpus is a different 180-module sample, so
its `506` is not directly comparable; by the same per-module rate, on the order
of 180 of those 506 are the same scaffolding literal, and the residual user
rule-1 population there is smaller than 506 but larger than `506 − 180`. Anyone
re-running that measurement should re-derive it against this compiler rather
than subtracting.

`gc_repsel_matrix.sh --arms all --pressure 8` was started and aborted during its
compile phase: a sibling agent was running the identical 21-arm matrix
concurrently and host free disk had fallen below the campaign floor. For a
change that emits identical code the IR comparison above is the stronger
statement, but the matrix columns are unmeasured and are recorded as such.

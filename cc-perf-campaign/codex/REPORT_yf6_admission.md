# YF6 typed-i1 admission report

## SHA

Implementation commit: `ae8e15aa28491a63312a59050b14b70d4831a82c`.

The implementation commit was pushed to
`fork/perf/typed-numeric-predicate`; `git ls-remote` resolved that branch to
the same SHA before this report-only child commit was created.

## Failing condition

The failure is return-type inference, not the erased-parameter proof, capture
analysis, typed-body safety walk, comparison floor, typed lowering, or a clone
instruction budget.

`crates/perry-hir/src/lower_types.rs:315` caps recursive expression-type
inference at 48 levels, and `infer_type_from_expr` returns `Type::Any` at
`crates/perry-hir/src/lower_types.rs:343-345`. The old `LogicalAnd |
LogicalOr` rule recursively inferred both children. YF6's 135 left-associated
`||` joins exceed that depth, so an inner left subtree became `Any`; the
logical unification propagated `Any` to the root. `infer_body_return_type`
then rejected that `Any` at `crates/perry-hir/src/lower_types.rs:923-933`, and
`crates/perry-hir/src/lower_decl/fn_decl.rs:372-383` consequently left
`Function.return_type` at its initial `Any` fallback. The smaller probe's 22
`||` joins remain below 48 and therefore infer `Boolean`.

At codegen admission, that metadata produces
`TypedCloneRejectionReason::ReturnTypeNotI1` at
`crates/perry-codegen/src/codegen/typed_abi.rs:1277-1279`.
`typed_i1_function_rejection_reason` only tries erased-predicate admission when
the declared-path reason is `ParamNotI1`
(`crates/perry-codegen/src/codegen/typed_abi.rs:839-842`), so YF6 never reaches
the override. Calling `erased_numeric_predicate_param_reps` directly would
also fail its Boolean-return rule at
`crates/perry-codegen/src/codegen/typed_abi.rs:1210-1220`.

All later rules pass once the return type is Boolean:

- `typed_param_rep_for_type` deliberately does not type `Any`
  (`crates/perry-codegen/src/codegen/typed_abi.rs:144-155`), after which the
  erased rule sees referenced parameter `q` and assigns guarded `F64`
  (`crates/perry-codegen/src/codegen/typed_abi.rs:1223-1239`). The real
  top-level function has no captures.
- `typed_i1_body_rejection_reason` accepts the one-return straight-line body
  (`crates/perry-codegen/src/codegen/typed_abi.rs:1705-1739`): every logical
  node is `And`/`Or`, and every comparison has `q` plus an integer literal as
  f64-safe operands.
- `numeric_comparisons_in_typed_i1_expr`
  (`crates/perry-codegen/src/codegen/typed_abi.rs:1191-1207`) counts 212, which
  clears `ERASED_NUMERIC_PREDICATE_MIN_COMPARISONS = 4` at line 1189.
- There is no per-function instruction budget in the typed top-level clone
  selection or typed-i1 lowering path.

## Change

`LogicalAnd`/`LogicalOr` return inference now flattens logical joins onto an
explicit worklist (`crates/perry-hir/src/lower_types.rs:357-388`, selected at
line 500). It applies the same sound rule as before: every leaf must infer to
the same non-`Any` type. The general depth-48 guard remains intact. A separate
512-node work cap bounds repeated inference cost and prevents the unbounded
O(n²) behavior that the original guard protects against, while covering YF6's
423 logical/comparison AST nodes.

`crates/perry-codegen/tests/yf6_admission.rs` parses and lowers the exact
2,274-byte YF6 fixture verbatim, appends a typed-string `codePointAt` caller,
and asserts:

- the inferred YF6 return type is `Boolean`;
- `YF6$typed_i1` exists with 76 `fcmp oge`, 76 `fcmp ole`, and 60 `fcmp oeq`;
- the clone has no `@js_rel_` calls;
- the generic fallback remains present;
- the public wrapper guards and calls the clone while retaining the fallback;
- the possibly-`undefined` `codePointAt` result enters through that guarded
  public wrapper.

Restoring recursive logical inference makes the inferred-return assertion and
clone assertions fail.

## Gates

`df -g /` reported 0 GB available, below the binding 12 GB threshold. No cargo
invocation was made, so neither required cargo gate was run:

- NOT RUN: `cargo test -p perry-codegen`
- NOT RUN: `cargo build --release -p perry`

Non-cargo checks completed: `rustfmt --check`, `git diff --check`, exact fixture
`cmp`, source operator counts (76 `>=`, 76 `<=`, 60 `===`, 76 `&&`, 135
`||`), and SHA-256 equality with the supplied source
(`ffa564fc9612a0a1011f60d343d43777d082437547af2679ecb1568f478d1051`).

## Perrymaster request

Rebuild the compiler from implementation SHA
`ae8e15aa28491a63312a59050b14b70d4831a82c` on the I7-view tree, compile the
cc bundle with I7-view's configuration, and verify:

```sh
nm <cc-binary> | grep -c 'perry_fn_cli_2_1_112_js__YF6\$typed_i1'
```

The identity must be `1`. Then collect the rows versus I7-view and the perf
draw, using marker `YF6` self from the 9.46% baseline.

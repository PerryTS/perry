### perf(codegen, runtime): a declared `string` is enough to pick the concat lowering, and a `static` counter stops re-interning its own key

Three changes to the same theme — Perry re-deriving at runtime facts it already had at compile time — plus the runtime guarantee that makes the first one safe.

Measured on the quiet M1 mini, best-of-5, against a clean build of `1ee158d27` (perry 0.5.1463). Both arms built with `-p perry -p perry-runtime-static -p perry-stdlib-static`; every corpus program's output verified byte-identical to `node --experimental-strip-types` (Node 26.5.1) with exit 0 before timing.

| bench | before | after | |
|---|--:|--:|--:|
| `concat_field` (new probe) | 0.2739 | **0.1556** | −43.2% |
| `concat_field_base` (its subtrahend) | 0.0969 | 0.0966 | — |
| **ns per concatenation** | **88.5** | **29.5** | **−67%** |
| `gc-handoff/apps/pipeline.ts` | 0.5163 | **0.4846** | −6.1% |
| `gc-handoff/apps/shapes.ts` | 0.1900 | **0.1833** | −3.5% |

No corpus regression: `churn` 0.4218→0.4242, `churn_alloc` 0.3733→0.3758, `push_cls` 0.3691→0.3693, `push_num` 0.1533→0.1520, `churn_read` 0.0242→0.0230, `cycles` 0.1953→0.1951, `deeplist` 0.1242→0.1244, `tree` 1.6455→1.6437, `tree_wide` 2.1170→2.1166, `retain` 0.3607→0.3603, `retain1` 0.1498→0.1477, `retain_wide` 0.4714→0.4711, `fib40` 0.4063→0.4065, `asyncpipe` 0.1329→0.1331, `interp` 1.5162→1.5159, `iso_miss` 1.9456→1.9433 (`checksum 437840 misses 0`). The six `*_real` globalThis-bootstrap arms stay within ±2% of their base arms, as they were.

#### 1. `js_string_concat_box` delegates a non-string operand instead of dropping it

It decoded both operands with `str_bytes_from_jsvalue(...).unwrap_or((null, 0))` — so an operand that was not a string became the **empty string**. `"ab" + 42` through this helper rendered as `"ab"`.

**This is latent on the pre-change compiler, not a user-visible bug.** Codegen only selected this helper when both operands satisfied the strict `is_definitely_string_expr`, so a `"ab" + 42` reaching it required a lying *`string`-declared local* — the one arm of that predicate which is already a declaration rather than a structural proof. `o.t + 7` on a `string`-declared field, and a `(string, number)` parameter pair, both routed elsewhere and answered correctly. What made the fix necessary is change (2): widening the operand test to accept a declaration is exactly what would have made the wrong answer reachable.

It was also the reason the concat fast path could never be selected from a type annotation in the first place: `lower_string_concat.rs`'s self-append lowering carries a whole `dother`/`cold` arm whose comment says, in as many words, that a lie has to be routed around this helper. It now forwards any non-string pair to `js_dynamic_string_or_number_add`, which is the full spec `+`. String+string is unchanged (including the ≤5-byte SSO result encoding); string+number concatenates with the number's decimal form; number+number **adds and returns a number**.

#### 2. `+` accepts a declared `string`, but only where (1) makes that free

`"shape:" + this.tag` — a string literal plus a field declared `string` — lowered to `js_dynamic_string_or_number_add`: a `RuntimeHandleScope`, four `root_nanbox_f64`s and two `ToPrimitive` calls, spent rediscovering what the declaration already stated. `is_string_expr` has trusted that same declaration for string *method* dispatch since #655; the concat path did not.

The new predicate is `is_declared_string_expr`, and it is deliberately **separate** from `is_definitely_string_expr` rather than an extension of it. Perry does not enforce annotations at runtime — the exact gap #7831 is closing on the numeric side — so a declaration is evidence, not proof. It therefore has exactly one consumer: the two-operand concat, which emits `js_string_concat_box`. After (1) that helper produces the dynamic-path answer for every combination of runtime values, so **the declaration selects a lowering and can never select an answer.**

Three neighbours deliberately keep the strict predicate, each because it *would* be able to change an answer:
- the one-sided `l ^ r` arm lowers through `js_string_concat_value` / `js_value_concat_string`, which take an already-unboxed `StringHeader*` and cannot tell a lie from a string;
- the N-way chain fold formats every part as a string, so an all-declared chain of numbers would concatenate where the spec adds;
- the `Map` string-key fast paths in `expr::math_simple` key a lookup on the claim.

#### 3. `type X = { … }` resolves its property types, like `interface X { … }` already did

`lower_type_alias_decl` files aliases in `module.type_aliases` while `lower_interface_decl` files interfaces in `module.interfaces`, and `static_type_of` only ever consulted the latter. An object-type alias is structurally interchangeable with an interface in TypeScript — same declaration, same runtime layout, same absence of any layout guarantee — and `type` is the form most application code reaches for. So `type Record = { kind: string; … }` proved **nothing** about `r.kind`, purely because the author wrote `type` instead of `interface`. Only a non-generic alias whose right-hand side is a closed object type answers; an alias to a `Named`/`Generic` type is left alone.

This is what makes `pipeline.ts`'s `makeTagger` (`prefix + r.kind`, 360,000 calls) reach the concat at all.

#### 4. `class_dynamic_prop_root_store` stops allocating a key it already has

Codegen emits `js_class_register_static_field` after every `Expr::StaticFieldSet`, so `Shape.made = Shape.made + 1` inside a constructor runs it **once per construction** — 144,000 times in `shapes.ts`. Each call did `str::from_utf8(…).to_string()` (a heap allocation, immediately dropped, because `HashMap::insert` keeps the *original* key), a `CLASS_DELETED_KEYS` probe, and an `entry().or_insert_with().insert()`.

The signature is now `&str` — every caller already had a borrowed value — and a store whose key exists updates the slot in place. The in-place arm also skips the deleted-keys probe, but only while **nothing has ever been deleted**; once anything has, the original sequence runs verbatim, so the pre-existing conflation between a deleted prototype key and a same-named static field (`class C { m() {} static m = 1 }`, both under one class_id) keeps whatever behaviour it had.

#### Tests

- `perry-runtime`: `concat_box_delegates_a_non_string_operand_to_the_dynamic_add` pins all four operand combinations plus the SSO result encoding; `repeated_store_updates_in_place_and_stays_readable` and `store_after_delete_clears_the_deleted_mark` pin both arms of the static-field store.
- `perry-codegen`: `alias_declared_string_field_takes_the_static_concat`, `field_absent_from_the_alias_keeps_the_dynamic_add`, and `alias_declared_number_field_is_deliberately_not_routed` — the last one asserts the numeric side is *unchanged*, so a future widening there has to be a deliberate edit to this test.

All in-crate unit tests, so they run in the per-PR `cargo-test` job rather than the nightly-only integration suites (#5960).

#### Probes

`gc-handoff/bench/concat_field.ts` and `concat_field_base.ts` are the ns/concat pair: identical programs, 2,000,000 `"shape:" + this.tag` concatenations, differing only in whether `describe()` concatenates or returns a same-length constant.

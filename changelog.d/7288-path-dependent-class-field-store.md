**Fixed** a byte-identical `.ts` file compiling to a **46x slower** object depending
on *where on disk it lives*. `benchmarks/suite/09_method_calls.ts` ran 83 ms
compiled inside the Perry checkout and 3,762 ms compiled anywhere else, and
`benchmarks/results/public-node-bun-v1.json` published the fast number (79 ms)
because `benchmarks/compare.sh` does `cd benchmarks/suite` first. A user
compiling the same file in their own project got the slow arm.

**The discriminator is strict mode, resolved by an upward directory walk.**
`perry_parser::file_is_in_esm_package_context` walks up from the source file for
the nearest `package.json`; `"type": "module"` makes an ambiguous-extension file
(`.ts`/`.js`) an ES module, and module code is strict code
(`lower_module_fn::module_has_strict_mode`, #6542). Perry's own root
`package.json` is `"type": "module"`, so *every* file inside the checkout is
strict and every file outside it — with no `package.json` above — is sloppy.
That determination is correct and matches Node; what was wrong was how much
codegen hung off it.

`put_value_static_property_fast_path` (`expr/proxy_reflect.rs`) barred sloppy
code from the entire class-field store route with three `if !strict { return
None; }` bails. The stated reason (#6542) is real but narrow: that route's
terminal fallback is `js_class_field_set_fallback` →
`js_object_set_field_by_name`, which throws unconditionally on a non-writable
slot — correct for strict `PutValue`, wrong for sloppy, where a rejected write
is a silent no-op. The bail discarded the **fast** arm to fix the **fallback**
arm. Sloppy `this.value = this.value + 1` fell all the way to
`js_put_value_set_dyn_ic`, a runtime call per iteration.

The fast arm never needed the bail. The #5093 inline precheck
(`emit_class_field_inline_precheck`) already rejects every receiver whose store
could be *rejected* — `OBJ_FLAG_FROZEN`, `OBJ_FLAG_HAS_DESCRIPTORS`, a
mismatched class id or keys token, a cleared typed-layout-intact bit — and every
value that is not a plain finite number, and the process-global gate is flipped
by any prototype-level descriptor install naming a declared field. A store that
reaches the raw slot is one that could not have been rejected in *either* mode,
so the fast arm is mode-independent by construction.

Sloppy `obj.f = <number>` on a declared `number` field of a known class now
emits that same precheck and the same raw slot store
(`property_set::try_lower_sloppy_class_field_raw_store`), and routes every miss
to `js_put_value_set(..., strict = 0)` — the sloppy-correct runtime the
surrounding `PutValueSet` lowering already used — instead of the throwing
by-name setter. No runtime change was needed. Scope is deliberately narrow:
raw-f64 (`number`) fields, receiver == target; boxed slots need the layout note
and write barrier the guard-call path emits and stay on the unchanged inline
caches, as do oversized modules that full-outline the whole diamond (#5334).

`09_method_calls` outside a checkout: **3,762 ms → 81 ms**, matching the
in-checkout arm (80–83 ms) exactly, so the two arms now agree and the published
baseline describes what users actually get.

**Two related findings worth recording.**

The issue's `compilePackages` lead was a red herring, and this explains it:
adding a `package.json` carrying a `perry` key *inside* the checkout flipped the
build to the slow arm not because of `compilePackages` but because Node stops at
the nearest package scope — a nested `package.json` without `"type": "module"`
ends the walk at a non-ESM scope. Confirmed directly: `{"name":"x"}` with no
`perry` key at all reproduces the flip (3,835 ms), and `{"type":"module"}`
outside the checkout gives the fast arm (83 ms).

**The gap suite structurally cannot see this class of bug.** Every
`test-files/*.ts` sits under the repo root's `"type": "module"`, so the whole
corpus compiles strict; `run_parity_tests.sh` already acknowledges this for Node
(it retries failed import-free globals fixtures as `.cts` to get script
semantics) but the compiler side has no sloppy arm under test. Any codegen
predicate keyed on `strict` is exercised in one state only.

Verified: a 20-case sloppy differential (inheritance chains, shadowed subclass
fields, accessors, string fields, `null`/`undefined`/boolean/object/BigInt
stores into a `number` slot, `2**53` / `-1e308` / denormal boundaries,
`preventExtensions`, `delete` then re-add, aliased writes, a 100k-iteration
megamorphic loop over 50 instances, `Object.freeze` *mid-loop*, and enumeration
order) is **byte-identical to Node 26.5.1 and to the pre-change compiler**, with
150 `class_field_sloppy_set` blocks in the emitted IR proving the new arm is
live; a separate 8-case frozen/non-writable/prototype-accessor probe matches
Node in both sloppy and strict mode. 24/24 `test_gap_class*` / `test_gap_object*`
tests byte-match Node. `cargo test -p perry-codegen --lib` 633 passed;
`native_proof_regressions` has the identical 4 pre-existing failures before and
after (249 passed vs 248, the extra pass being the new test). The strict arm's
emitted IR is unchanged on 22 of 25 in-checkout files; the 3 that differ are
**self-nondeterministic** — the unmodified compiler produces three different
hashes across three runs of the same input (#7303) — and their output still
matches Node.

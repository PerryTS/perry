# #7963 — `Object.defineProperty`'s own receiver / key / descriptor-field window

Working notes for the `gc/7963-define-property-rooting` branch. Written
incrementally; the PR body is the summary, this is the audit trail.

## The class

Raw NaN-boxed values and raw heap pointers held in ordinary Rust locals across
calls that can allocate. Neither shadow slots nor temp roots nor reachable from
any registered scanner, so an evacuating minor can neither keep them alive nor
rewrite them. `scripts/gc_root_dominance_check.py` reads emitted LLVM IR, so it
is structurally blind to the whole class.

#7949/#7962 closed the *container* shape (`Vec<f64>` accumulators). This is the
`obj` / `key_str` / `DescView` shape #6949's scope note names and defers:

> `js_object_define_property` also holds `obj` / `descriptor_value` and the six
> raw `JSValue`s inside `DescView` across its own later `js_string_from_bytes`
> calls, and `obj_value_has_own_key` holds `keys` / `key_str` across a
> `js_array_get` walk that can materialize a lazy array.

## The pristine fault, reproduced and localized

`test-files/test_gap_gc_define_property_descriptor_rooting.ts` under the witness
configuration, on a pristine `origin/main` release build (`a769fafc6`,
`PERRY_NO_AUTO_OPTIMIZE=1`, `PERRY_RUNTIME_DIR` pinned to that build's `.a`
pair):

```
exit 138, stdout stopped after "definePropertyOneAtATime ok"

[gc-fromspace-protect] FAULT: signal 10 at 0x39f9556083a
  This address is RETIRED FROM-SPACE. ...
  block=0x39f95560000 +2106 retired_bytes=4200 retired_by_minor=#135
  last-known object: user_ptr=0x39f95560840 obj_type=2 size=56
```

`obj_type=2` is `GC_TYPE_OBJECT`. The program dies in arm 3 — the descriptor
bag whose fields are ACCESSORS, so `desc_read_field` runs user JS inside
`js_object_define_property` — which is precisely the window #6949's scope note
defers. The instrument is live: 135 from-space page-sets were retired before the
fault. The same program on this branch exits 0.

**Do not attribute a fault from the census line alone.** The first draft of this
probe faulted with a *different* signature (`obj_type=3`, `GC_TYPE_STRING`, at
`user_ptr + 4` — which is `StringHeader::byte_len`, so it really was a stale key
string) and it was tempting to read that as the defineProperty key. It was not;
see the next section. Only a symbolicated backtrace settled it.

## A second defect the first draft of the probe walked into

The probe originally compared each arm inline —
`console.log("x", observed() === expected() ? "ok" : "BAD")`. Under the witness
configuration that faults on a pristine build **and on this branch**, in
`js_jsvalue_equals` <- `js_eq` <- `main` (symbolicated against an unstripped
`perry-dev` runtime with `PERRY_DEBUG_SYMBOLS=1`). The left operand is an SSA
temporary live across `expected()`, which allocates through several loop
back-edges and therefore collects: the temporary names from-space. That is a
**codegen** root-dominance defect — the class
`scripts/gc_root_dominance_check.py` exists for — with nothing to do with
`Object.defineProperty`, and it is filed separately.

Binding both sides to `const` first removes it from this program, and only then
does the A/B separate. Worth recording as method: the FIRST fault a witness
program produces is not necessarily the defect you are hunting, and the census
line (`obj_type`, `size`) is a hint, not an attribution. Symbolicate.

## Sites fixed

### 1. `object/object_ops/define_property.rs` — the ordinary-object arm

`obj` (`*mut ObjectHeader`) and `key_str` (`*mut StringHeader`) were resolved
once near the top and then carried, raw, to the end of the function — through
`define_array_property`, `enforce_define_property_invariants`,
`obj_value_has_own_key`, `ensure_key_in_keys_array`,
`clone_closure_rebind_this`, `define_property_force_store_value`, and every
`desc_has_field` / `desc_read_field` (each allocates a field-name string, and on
an accessor-backed descriptor field runs USER JS). `obj_value`,
`descriptor_value` and `key_value` were rooted only across the initial
`js_string_coerce` and then read as plain locals for the rest of the body.

The receiver is the worse half: `obj as usize` is the OWNER KEY of the
per-property descriptor side tables (`set_property_attrs`,
`set_accessor_descriptor`, `accessor_descriptors`), so a stale receiver files
the attributes and accessors under a dead address where the matching read can
never find them — a silent wrong answer, not a crash.

Fixed by rooting all five and introducing an `across!` macro that is the only
way to name any of them across a call: it runs the call first and rebinds all
five from their roots afterwards, so a pre-collection address is never
nameable. No new bare `get_raw_*_ptr` sites — `RuntimeHandle::across_mut` is
what the `scripts/raw_handle_debt.py` ratchet asks for, and the file's count
went 3 → 2.

Also rooted inside that arm:

* the descriptor's `get` / `set` field values, which spanned
  `ensure_key_in_keys_array` and the first of two `clone_closure_rebind_this`
  calls;
* the existing accessor's `get` / `set` closure bits, which are written back
  into the (GC-scanned) accessor table when the redefining descriptor omits a
  field, and which spanned the same two allocating calls;
* the class-prototype mirror's method value, which spanned
  `descriptor_enumerable` (two more descriptor field reads).

The three inner `RuntimeHandleScope`s (closure arm, typed-array arm, ordinary
arm) were collapsed into ONE scope created before `try_decode_descriptor`. That
is deliberate: the scope has to outlive the `DescView` handles, and an inner
scope dropped while an outer one is still taking handles truncates the outer
container's newest entries (the hazard documented on `gc::RootedValues`).

### 2. `object/object_ops/descriptor_helpers.rs` — `DescView`

`DescView` held six raw `JSValue`s read at decode time and handed them back at a
dozen points spread over the rest of `js_object_define_property`. The stale word
was not merely read — it was **stored into the receiver**
(`define_property_force_store_value`) or into the accessor table. Each present
field is now a `RuntimeHandle`, so `read` returns the post-collection address;
absent fields hold no handle and read `undefined` as before. `DescView` gained a
`'scope` lifetime; `try_decode_descriptor` takes the scope.

`validate_nonconfigurable_redefine`'s per-field arm (`desc_view == None`) also
allocated a field-name string per probe while holding `desc_ptr`, the current
value being compared, and the current accessor's closure bits. All three are now
rooted and re-read; `desc_ptr` is re-resolved *after* the allocation that
precedes each read.

### 3. `object/reflect_support.rs` — `obj_value_has_own_key`

The final keys-array walk held `keys` and `key_str` across
`crate::array::js_array_get`, which materializes a lazy array and therefore can
allocate. Both are rooted and re-read per iteration. The
`string_coerce_is_inert` shortcut around the scope was dropped: the walk needs
the same scope whatever the key's shape, so skipping it bought nothing. File's
raw-handle count went 4 → 3.

## How the fix is proven

`crates/perry-runtime/src/gc/tests/rooted_define_property.rs`, three tests, all
under `CopyingNurseryTestGuard` + `suppress_automatic_triggers`:

1. `define_property_lands_on_the_receiver_a_descriptor_getter_moved` — the
   end-to-end proof, through the real `#[no_mangle]` entry point. The descriptor
   bag's `value` field is an ACCESSOR whose getter forces a copying minor (which
   is what pushes `try_decode_descriptor` onto the spec-general path, so
   `desc_read_field` runs user JS mid-define). It asserts, in order:
   `copied_objects > 0`; the **receiver's address changed**; the **key string's
   address changed**; then that the property reads back the getter's payload
   bytes; then that `get_property_attrs` finds the entry **at the live
   address**. The last assertion is the one that catches a stale receiver, since
   the attribute table is keyed by address.
2. `desc_view_field_values_are_rooted` — `try_decode_descriptor`'s fast path,
   the `DescView` half: decode, force a copying minor, assert the field's
   address changed and it still reads the original bytes.
3. `unrooted_receiver_copy_still_names_from_space` — the sabotage arm. The same
   address held in a plain Rust `usize` (exactly what pre-fix
   `js_object_define_property` carried) keeps naming its pre-collection value in
   the same cycle in which the rooted handle to the SAME object is rewritten.
   This is what makes (1) and (2) non-vacuous.

### Sabotage verification (fix committed first)

See "Sabotage run" below.

### Compiled probe — the A/B

`test-files/test_gap_gc_define_property_descriptor_rooting.ts`, three arms: an
allocating `Object.groupBy` first arm (to retire from-space blocks), a
hand-written `Object.defineProperty` loop, and a loop whose descriptor bag
carries three allocating accessor getters (so `desc_read_field` runs user JS
mid-define).

Both arms compiled with `PERRY_NO_AUTO_OPTIMIZE=1` and `PERRY_RUNTIME_DIR`
pinned to their own `.a` pair; the fixed pair's mtimes were confirmed to have
moved after the edit.

| build | witness configuration | default |
|---|---|---|
| pristine `origin/main` (a769fafc6) | **exit 138**, `[gc-fromspace-protect] FAULT` at `block+2106`, `retired_by_minor=#135`, `obj_type=2` (a receiver `ObjectHeader`), stdout stops after arm 2 — i.e. it dies in the **descriptor-getter** arm | exit 0, byte-identical to node 26.5.1 |
| this branch | **exit 0**, `[gc-schedule] done: safepoints=301 scheduled_collections=301 copying_minors=301 moved_objects=110912 loop_polls=8175` | exit 0, byte-identical to node 26.5.1 |

Instrument liveness is reported rather than assumed: the pristine arm retired
135 from-space page-sets before it faulted, and the fixed arm ran 301 copying
minors moving 110,912 objects.

## Not covered by a moving test

* The `obj_value_has_own_key` keys-walk fix (site 3) is by inspection: the
  allocation there is a lazy-array materialization, which the unit harness has
  no cheap way to force. Stated rather than glossed.
* An accessor-install test (a collection between the descriptor's `get` and
  `set` reads) was written and dropped: it kept faulting inside the harness's
  own setup rather than in the code under test, and a test that fights the
  harness is not evidence. The window it targeted is covered end-to-end by the
  compiled probe's third arm.

## Acceptance corpus

All 37 `test_gap_gc_*.ts` plus the 5 `test_gap_{proxy,reflect}*.ts` programs,
compiled against the fixed runtime (`PERRY_NO_AUTO_OPTIMIZE=1`,
`PERRY_RUNTIME_DIR` pinned), byte-compared to node 26.5.1 in the default
configuration AND under `PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`:

```
==== pass=42 fail=0 node-skip=0 quarantine-live=20 ====
```

`quarantine-live=20` is the honest half: only 20 of the 42 programs ran a
copying minor at all, so for the other 22 the protected arm is a
no-regression check and not a rooting witness. (#7962 reported the same 20-of-41
split.)

## #7964 — verdict: COMPILER GAP, not a stale pin. Do not regenerate.

Reproduced on this branch (the change is runtime-only, so this is the state of
`main`): `test-files/gc-dep-corpus/main.ts` fails to link with **45 distinct**
undefined `_perry_fn_node_modules_zod_...` symbols.

Evidence for "compiler gap":

1. **The mangled module in every failing symbol is the BARREL, not the
   definer.** `_perry_fn_node_modules_zod_src_v4_core_index_ts__NEVER` says
   Perry believes `NEVER` is *defined by* `core/index.ts`. `core/index.ts` is
   16 lines of `export * from …` / `export * as ns from …` and defines nothing.
   The actual definition is `core/core.ts:13`.
2. **The consumer shape is a named re-export from an `export *` barrel.**
   `v4/classic/external.ts` does
   `export { globalRegistry, config, $brand, clone, prettifyError, … } from "../core/index.js";`
   — every one of those names reaches `core/index.ts` only through
   `export * from "./core.js" | "./api.js" | "./registries.js" | "./errors.js"`.
   Perry emits the reference and never the forwarding definition.
3. **It is not a type-only-export leak.** The failing set mixes `const` exports
   (`NEVER`, `globalRegistry`, `$brand`) with `function` exports (`config`,
   `clone`, `prettifyError`, `_gt`, `_minLength`, …), so "erased type export
   still got a symbol" does not explain it.
4. **The pin has not drifted.** `package.json` asks for `zod@^4.3.5` and
   `node_modules/zod/package.json` is `4.3.5` — exactly the version named in
   #7964. Nothing in the corpus pins a commit that could have moved underneath
   it.

Two minimal reproducers I built (in `/tmp`, deliberately not added to the repo —
see the collision note below) both LINK, so the gap needs more of zod's shape
than one hop: `leaf.ts` → `barrel.ts` (`export *`) → `top.ts` links, and adding
a `bridge.ts` (`export { X } from "./barrel.js"`) still links. The remaining
candidates are the barrel's multi-source `export *` set, its
`export * as ns from`, and the `core/index.ts` ↔ `core/api.ts` cycle.

**Collision:** `/Users/amlug/projects/perry/wt-codex-7964` is another agent's
worktree on branch `fix/7964-zod-star-reexports`, already carrying uncommitted
edits to `perry-hir/src/lower/module_decl.rs`, `perry-hir/src/dynamic_import.rs`
and `perry-codegen/src/codegen/helpers.rs`, plus fixtures
`test-files/test_gap_export_star_variable_reexport.ts` and
`test-files/_helpers/issue_7964_{leaf,barrel,bridge,top}.ts` — the same four-file
shape I arrived at independently. I stopped at the verdict rather than shipping a
second implementation of the same fix.

## #7803 — still blocked

The corpus does not link, so #7803's reproducer still cannot be run. Nothing on
this branch changes that (the fix is runtime-side; the failure is in module
lowering/codegen). The candidate connection stands and is now slightly stronger:
zod's `Object.defineProperties` calls
(`src/v4/core/util.ts:316`, `src/v4/classic/errors.ts:28`) go through the helper
#7949 fixed, and its `Object.defineProperty` calls go through the window this
branch fixes; #7803's `Cannot read properties of undefined (reading 'toString')`
is what a stale key or a stale receiver in either loop produces. **Candidate,
not confirmed** — retest once #7964 lands.

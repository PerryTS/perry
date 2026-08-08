### Layer 1 rooting migration, slice 2 — `expr/instance_misc1.rs` + `expr/logical_collections.rs` + `lower_call/property_get/map_set.rs` (#7615)

All three modules now root every operand through `crate::rooting` and are listed
in `MIGRATED_MODULES`; none names `expr::temp_root`. Follows the template
(#7617) and slices 1a (#7618) / 1b (#7620).

**Two combinators, each arriving with its callers.**
`rooting::with_operands_rooted_across_call` roots a group across a window whose
`across` step is an **emitted runtime call** rather than a lowered expression.
`re.test(s)` / `re.exec(s)` unconditionally emit `js_jsvalue_to_string_coerce`,
which allocates and, on an object argument, dispatches a user `toString`; there
is no `Expr` for `any_may_trigger_gc` to read, and deriving the window from the
`string` operand answers *false* for a plain local, dropping the root at exactly
the site #7154 faults at. The window is therefore stated, conservatively, rather
than derived — the precedent being `temp_root::guard_store_operand_across`, which
has taken a `bool` for the same reason since #7201. `operand_protection` still
decides *how* each operand is protected, and all three `with_operands_rooted*`
forms now share one implementation so the family cannot grow three orderings.

`rooting::with_rooted_accumulator` is the operand group's mirror image: an
operand is lowered once and read once, an accumulator is written, read, rewritten
and read again with arbitrary user code lowered between the writes. It enforces
what a raw handle cannot — the accumulator never exists as a register held across
an emission. Consuming calls re-read it as part of being emitted, a helper
returning a fresh address publishes it straight back (`advance`), and the one
point where a register escapes is `finish`, which runs below the last collection
point and above the release; both closures' `?` paths release. `RootedSlot` also
gained a `Repr`, so a boxed slot can no longer be read as a raw pointer — that
choice used to live at each call site, where a mismatch is a silent miscompile
rather than a type error.

**Windows closed.** `with (o) { x = f() }` materialised the receiver *and the
interned property key* above the RHS and used them below it; the key is a load
from a `__perry_init_strings_*` handle global that evacuation rewrites, so the
write landed under a garbage key on a stale receiver (#7114 in a second arm).
`arr.filter/some/every(cb)` held the array across the callback's `js_closure_new`
— #7620's `find*` finding in three arms that live in a different file. Two raw
accumulators were unrooted outright: `Math.min`/`Math.max` with three or more
arguments threads an `ArrayHeader*` through `js_array_push_f64` across each
remaining argument's lowering, and the static-headers path of `fetch(u, {headers:
{k: f()}})` does the same with `js_object_alloc` — both #7154's `ObjectSpread`
bug, and both beyond what #7280's `root_reload` can repair because the stale
value is an `i64` derived above the window. `fetch`'s `url`/`method`/`body` sat
in registers across the headers construction and across
`js_fetch_headers_to_json`, which enumerates a program-supplied value's own
properties and so can re-enter user code. `"k" in process.env` and
`JSON.parse(<literal>, <closure>)` each held a string literal's handle across an
intervening allocating call, repaired by one re-emitted `load` and no runtime
call. Plus the ordinary operand-to-operand windows: `delete o[k]` (both forms),
`x instanceof <dynamic>`, `path.join` / `path.win32.*` / `path.relative` /
`path.basename(p, ext)`, `arr.includes` / `.splice` / `.join` / `.slice`,
`Array.from(it, fn)`, `Object.groupBy` / `Map.groupBy`, `s.match` / `s.matchAll`,
`JSON.parse(t, reviver)`, `parseInt(s, r)`, `new RegExp(p, f)`,
`Array.prototype.<m>.call(like, …)`, `map.delete(k)`, `a[i]++` and
`process.nextTick(cb, …args)`.

**Scoped honestly.** `map_set.rs` was already hand-rooted end to end (#6970), so
its migration is a translation and not a repair — its IR is byte-identical. What
it buys is that the release stops being a statement a later edit can move into a
branch, the shape #7462 shipped in `URLSearchParams.delete`, the direct sibling
of these arms. `ObjectSpread` and `Object.assign` were likewise already rooted and
become the accumulator with no behavioural change.

**Verified locally** (the CI backlog is deep, so this is the evidence), with the
#7622 double-compile control run *first*: clean on the probe set (172/172) and
reproducing 5 ordering-only permutations on the 149-module corpus, so nothing was
attributed before nondeterminism was excluded. IR over 4 purpose-built probes:
153/172 functions identical, `p1_mapset` byte-identical throughout, net delta
root plumbing only with nothing deleted and `js_write_barrier_root_nanbox` the
sole call-target change. Over the `gc-root-dominance` corpus: 2436/2452
identical; of the 16 diffs, 11 are root plumbing and 5 ordering-only — 4 in the
control set and the fifth pinned by six repeat compiles with the *baseline*
binary. `gc-root-dominance` green in both gated modes with an empty allowlist
(0 violations, `--seeded-violations 40` at 40/40, `--unrooted-allocas` 0 over
7867 gc-capable allocas), all four checker static audits pass, and root stores
went 9826 → 9846 over the identical corpus, so the gate's subject is
demonstrably live. `cargo test -p perry-codegen --lib` 691 pass and `--doc`'s two
`compile_fail,E0499` arms still reject; `-p perry-runtime --no-fail-fast` 1886
pass (one #7365 timing flake, green on rerun). 86 gap tests over 18 family
filters with identical verdict sets on both arms. All probes byte-identical in
stdout and exit code on both arms and again under `PERRY_GC_ZEAL=1
PERRY_GC_PROTECT_FROMSPACE=1`. Ledger sabotage run per module — `map_set.rs`,
`logical_collections.rs` and `instance_misc1.rs` each turn the ledger red naming
both injected lines. Unlike slices 1a and 1b these three named `expr::temp_root`
before the migration, so the ledger line is load-bearing on the committed source
and not only under sabotage.

**Deliberately not closed here.** `Expr::IndexUpdate` (`a[i]++`) still holds its
re-read receiver and index across `js_dyn_index_get`, `js_to_numeric` and
`js_numeric_step`, three calls that can each run user code, before
`js_dyn_index_set` consumes them. Closing it needs a *per-use* re-read inside the
body rather than one group-wide re-read — a different combinator, which per the
template's rule should arrive with the slice that needs it. The
operand-to-operand half is closed. Filed separately, as is a pre-existing SIGABRT
in `test_gap_fetch_request_from_node_incoming_message` that reproduces on a
pristine `main` build and is absent from `known_failures.json`.

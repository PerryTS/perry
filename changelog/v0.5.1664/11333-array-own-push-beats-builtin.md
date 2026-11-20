An own `push` now beats `Array.prototype.push` on a proven array, closing
#11021 and the last of #10943's 37 cases. `const a = [1]; a.push = (x) =>
"own:" + x; a.push(9)` printed `2` (the builtin appended and returned the new
length); it now prints `own:9` and leaves `a.length` at 1, as Node does. The
same holds through every receiver spelling that lowers to `Expr::ArrayPush` —
a module global, a function local, a captured or boxed binding, a `number[]`,
a pre-growth alias, `Object.defineProperty` data and accessor properties, and
a value-discarded `a.push(x);` whose own method must still run. A borrowed
builtin (`a.push = Array.prototype.push`) and an unrelated named property
(`a.foo = 1`) still take the builtin, and an own non-callable `push` throws
`TypeError` (the `array.push` row of `test_parity_11006_noncallable_own_builtin`
was red on main and is green now).

**No diamond, and no cost on the inline store.** #10958 left `push` out of the
own-override gate because a diamond around it cost +94 instructions per call,
and because an array was believed to record nothing in its header the inline
push tier could test. It does: every install of an array's own named property
arms `OBJ_FLAG_ARRAY_DESCRIPTORS` (0x400) — both storages of
`array_named_property_set` (the pairs reserve and the full-array fallback
table) and every `Object.defineProperty` route — the bit is monotone and
`js_array_grow` carries it, and every inline push tier's admission mask
already tests it (`0x407`, `0x3C07`, `0xF487`). Checked two ways rather than
read off the code: the runtime tests assert the bit after an install into
each storage, and the issue's own `const a = [1]; a.push = fn` row goes green
with the inline tier untouched — had the bit been clear it would have taken
the inline store and still printed `2`. So an array that owns `push` can never
take the inline store; the bug was that every slow arm then called
`js_array_push_f64_spec`, which returns the new head and has the expression's
value recomputed from the length, so no bail from it could carry an own
method's return value.

`js_array_push_f64_spec_or_own` (`perry-runtime/src/object/own_override.rs`)
has two exits instead, selected by an i32 out-flag: the new head (exactly the
old call, written back and measured as before), or the METHOD's return bits,
which `OwnPushJoin` (`perry-codegen/src/expr/array_push_own.rs`) phis in as the
expression's value with no write-back and no recomputed length. All five slow
arms use it: the #7634 spec-order arm, the typed-feedback numeric fallback, the
forwarded arm, the realloc arm and the local tail. The runtime answers the
common case from the same single header probe `js_array_push_f64_spec` already
made (`push_spec_if_plain`, split out of it), and asks the precise question —
does the live head own `push` in its accessor descriptors or named properties,
by bytes, without allocating — only when the bit is set. The own method is
resolved and called through `own_override`'s resolve/invoke halves, now split
so a caller with its own proof skips `PERRY_OWN_NAMED_PROP_INSTALLED` (which
`js_array_set_string_key`'s direct install path never arms), and the resolve
half classifies the value it already read instead of performing `Get` again, so
an own accessor's getter runs once.

Instructions per iteration (callgrind, `(Ir(2N) - Ir(N)) / N`, the shipping
`release` profile — thin LTO, one codegen unit — main -> this branch): hot
`a.push(i)` 42.234 -> 42.232; consumed `s += a.push(i)` 154.215 -> 152.238;
`a.push({v: i})` 1987.783 -> 1988.531 (main's own run-to-run spread is ~1); a
captured receiver (the local tail, where EVERY push is the new call) 529.259 ->
529.254; `a.indexOf(x)` 477.250 -> 477.250; element-read control 15.031 ->
15.031. Three things were needed to get there, each measured: the plain case
shares `js_array_push_f64_spec`'s single header probe (`push_spec_if_plain`,
`inline(always)`) instead of probing twice (+46 on the captured row); everything
past it lives in a `#[cold]` out-of-line function, because a handle scope and
the resolve/invoke halves inlined into the entry gave it a frame of its own
(+28); and the own-exit branch carries `llvm.expect.i1(false)`, without which
the new blocks cost an object-push loop three register moves per iteration.
(With 16 codegen units a `#[inline]` helper in `buffer_receiver_dispatch`,
which this change does not touch, was outlined and read as +16 on `indexOf`;
the shipping profile shows no such difference, and the generated IR is
identical apart from one `declare`.)

`a.push()` is not an `ArrayPush` — HIR folds a zero-argument push to a
`NativeMethodCall` so a frozen array still throws — and is a call on both arms
anyway, so it joins #10943's folded-node diamond (`folded_builtin_override.rs`)
at no inline cost.

Still open, because they do not lower through a guarded push: `a.push(...xs)`
(`ArrayPushSpread`), and `a.push(x, y)`, which HIR desugars into one
`ArrayPush` per argument — so with an own `push` it now calls the method once
per argument (Node calls it once with both) where it previously ran the builtin.
And the method is resolved AFTER the argument, as perry's push lowering has
always evaluated the argument first (#7634): an argument that itself installs,
replaces or deletes `a.push` sees the method as the argument left it, where
Node resolved it before. Of those, only installing an own `push` on an array
that had none newly differs from main; resolving first would put a header
probe ahead of every call-bearing argument, `out.push(f(x))` included.

Tests: `expr/array_push_own_tests.rs` asserts on emitted IR that both inline
slow arms and the spec-order arm call the own-aware push and never the bare
one, that the own exits reach a `phi`, that `apush.inbounds` carries no trace of
the exit, and that every admission mask (pointer, number, string pushes)
includes 0x400 — both sabotaged (a slow arm reverted to the bare call; the mask
narrowed to `7`) and seen to fail. `a_metadata_selected_add_keeps_the_runtime_
number_guard` now pins the numeric fallback to the own-aware call.
`object/own_override_push_tests.rs` covers the plain and unrelated-property
builtin exits, the probe on both named-property storages and through a
forwarded alias, the own exit's value and non-append, and the non-callable
throw. `test_parity_own_override_beats_builtin.ts` gains 14 rows across the
tiers, all red on main and byte-identical to Node here. No version bump.

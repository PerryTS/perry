### Fixed

**A declared `string` no longer picks the `+` operator (#7837).** `is_definitely_string_expr` answered `true` on the strength of an erased TypeScript annotation, and `+` then chose string concatenation from it. Perry does not enforce declared types at runtime — CLAUDE.md says so under Known Limitations — so `const s: string = (42 as any)` really does put a number in the slot. Thirteen shapes came out silently wrong, exit 0, no diagnostic; #7835 has since fixed four of them (the ones routed through `js_string_concat_box`, which it made total). These nine were still wrong on `ab1bd464b`:

| shape | Node | before |
|---|---|---|
| `s + 7` | `49` | `427` — concat chosen where the spec adds |
| `7 + s` | `49` | `742` |
| `s + true` | `43` | `42true` |
| `a + b + "x"` (N-way fold) | `141x` | `4299x` |
| `a + b + a` | `183` | `429942` |
| `const u = s; u + 7` | `49` | `427` |
| `(c ? s : "q") + 7` | `49` | `427` |
| `arr.slice(0) + 7` | `1,27` | `` (empty) |
| `f(a: string, b: number)` through a function value | `49` | `427` |

The last two are the same premise wearing different clothes. The `.toString()` / `.slice()` / `.replace()` … arm of the predicate matches on the **method name alone**, with no look at the receiver — `Array.prototype.slice` returns an array. And a `string` PARAMETER is live too: the first triage of this bug called parameters clean because a direct call gets inlined, which erases the annotation; reached through a function value the defect is there.

The policy, matching #7831 on the numeric side: **a static type may select a lowering, never an answer.** It is applied in the one place each site can afford it.

- **Helpers that receive both operands NaN-boxed can be made total, and #7835 did that**: `js_string_concat_box` forwards a non-string pair to `js_dynamic_string_or_number_add` rather than decoding it as the empty string.
- **The one-sided `l ^ r` arm could not be fixed that way**, because codegen unboxes the string operand to a `StringHeader*` before the call and the tag is gone by the time `js_string_concat_value` sees it. When the operand's string-ness is declared-only it is now passed NaN-boxed to `js_string_add_value` / `js_value_add_string`, which test the tag and then either run the identical fused single-allocation concat or fall through to the spec's `+`.
- **The N-way chain fold** formats every part as a string, so it reproduces the source tree only when the FIRST node really concatenates. It now requires a *proven* string in the head pair; a chain that fails that falls through to the pairwise lowering, which resolves each node from the runtime tags.

A new predicate, `string_value_is_runtime_guaranteed`, separates the two kinds of evidence `is_definitely_string_expr` had been mixing: a literal, `String(x)`, `JSON.stringify`, `path.join`, `os.arch()` and friends *construct* a string, while a `LocalGet` and a receiver-blind method name only *claim* one. Its whitelist is deliberately closed — an arm nobody has classified answers "claim" and gets guarded, because that costs one predictable compare while the other default costs a wrong answer.

**Cost: none measurable, and it is provable rather than sampled.** Compiling all 19 corpus programs with the base and the fixed compiler against the *same* runtime archives produced LLVM IR that differs by exactly two lines — the two `declare` statements for the new helpers. Zero call sites moved, zero folds were lost, and `"prefix" + i` keeps its fused concat because a literal is a proof. The guard lands only on reads the compiler could prove nothing about, and it lands as one compare inside a call that already allocates, so there is no codegen diamond and no phi for LLVM to lose an optimization to.

Still open, filed as #7841: the `s += x` **self-append** lowering has the same defect from the same premise (`let c: string = (42 as any); c += 1` gives `"421"`, not `43`). It lives in `lower_string_self_append`, not in `binary::lower`, its fix has to move a tag test above a `ToString` that has observable side effects, and it sits on the load-bearing O(n) string-builder path — so it wants its own change and its own measurement rather than a rider on this one.

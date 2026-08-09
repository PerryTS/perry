### `arr.push(f())` evaluates the receiver before the argument again (#7634)

ES2024 evaluates the `MemberExpression` `arr.push` to a Reference **before** the
argument list, so the push lands on the array `arr` named at that moment. Both
arms of `crates/perry-codegen/src/expr/array_push.rs` lowered the argument first
and read the receiver afterwards, so an argument that rebound the receiver
redirected the push onto its replacement:

```ts
let a: number[] = [1];
function f(): number { a = [9]; return 2; }
a.push(f());
console.log(JSON.stringify(a));   // node: [9]   perry: [9,2]
```

`arr.push(...g())` diverged the same way. Both now match `node 26.5.1`
byte-for-byte, along with `push`'s own result (the new length of the array it
pushed onto) and the aliasing case where another binding keeps the array the
push landed on. Covered by `test-files/test_gap_7634_push_receiver_order.ts`.

**The fix is gated on the divergence being observable, and that is the point.**
A blanket reorder makes the receiver live across the argument on *every* push —
so every `rows.push({...})` and `out.push(f(x))` would gain a temp-root
push/re-read/truncate, on what #7511 measured as the hottest store family in the
compiler. `push_receiver_is_rebindable` is the as-if test: the two orders name
the same array unless the argument assigns the receiver's id itself, or the
binding is **boxed** (`collect_boxed_vars`' rule is "captured AND mutated", so a
captured-but-never-assigned array stays on the fast path) or a module global
*and* the argument can reach a collection point. When it answers `false` — the
hot shape — the historical lowering is kept with its inline tiers and its
`Reuse` verdict, and the emitted IR is unchanged: a `--trace llvm` of a
1000-iteration `out.push(mk(i))` loop plus a captured `rows.push(i * 2)` arrow
contains zero `call i32 @js_gc_temp_root_push` and zero spec-order blocks.

When it answers `true`, the spec fix and the rooting fix are one change: the
receiver becomes an operand of `rooting::with_operands_rooted_across`, rooted
before the argument and re-read after it. `operand_protection` supplies `Root`
rather than `Reload` for exactly the reason this bug exists — re-deriving a
local or a module global would observe the argument's assignment.

**One thing the issue did not anticipate:** the reorder alone is not sufficient.
Every fast tier publishes the reallocated array head back into the binding
unconditionally, and once the argument may have rebound that binding the store
lands on the wrong array (`a.push(f())` would overwrite `[9]` with the grown
`[1,2]`). The spec-ordered arm therefore skips the inline tiers and guards its
write-back on the binding still naming the array that was pushed onto; when it
does not, the store is skipped and aliases stay valid through the forwarding
pointer `js_array_push_f64` installs (issue #233) — the same mechanism that
already keeps `const x = a; a.push(1)` correct.

The five-way storage write-back chain (boxed capture / boxed local / capture
slot / local alloca / module global, with #5459's fall-through) was duplicated
between the two arms and is now one `emit_push_writeback` shared by three
callers.

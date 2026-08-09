### `pointer_locals` stops typing every `Uint8ArrayGet` as a Number (#6998)

`collect_pointer_typed_locals`'s `expr_value_type` classified
`Expr::Uint8ArrayGet` as `Type::Number` with no regard for the key kind, so
`const it = u8[Symbol.iterator]` bound a value the collector had proven
non-pointer — and a non-pointer local gets **no shadow slot**, i.e. it is not a
GC root.

**Reachability, which the issue explicitly left open, is now established on the
emitted HIR rather than argued.** `lower/expr_member/member_tail.rs` folds every
non-STRING key on a `Uint8Array`/`Buffer`-typed local onto this node, and a
symbol key is not a string, so `--print-hir` gives exactly:

```
Let { id: 1, name: "it", ty: Any, mutable: false,
      init: Some(Uint8ArrayGet { array: LocalGet(0),
                                 index: SymbolFor(String("@@__perry_wk_iterator")) }) }
```

The arm now answers `Some(Type::Number)` only for a **structurally** numeric key
— the same `index_is_definitely_numeric` proof the `IndexGet` typed-array arm
next to it already uses — and `None` otherwise. Structural on purpose: a
`number`-declared index local is not evidence, because Perry does not enforce
annotations, and `expr_is_known_non_pointer_shadow_value`'s sharper three-part
test needs an `FnCtx` this collector runs before. `None` is the conservative
direction: the local keeps a slot the collector rewrites harmlessly. The
byte-read arm — the one #6996 paid to keep free — is unchanged, and
`a_numeric_keyed_uint8array_read_still_pays_no_slot` pins that.

**Found while verifying it: the runtime consequence is currently masked by a
second, behavioural defect, and that is worth more than this fix.** Five other
collectors (`integer_locals`, `i32_locals`, `int_valued_ta_locals`,
`not_bigint_locals`) and `type_analysis/numeric.rs` make the *same*
unconditional "a `Uint8ArrayGet` is a number" assumption, and they force the
i32-context lowering, so a non-numeric key on a typed-array-typed local reads a
**byte** instead of the property:

| | node 26.5.1 | perry |
|---|---|---|
| `typeof u8[Symbol.iterator]` | `function` | `number` |
| `const k: any = "byteLength"; u8[k]` | `4` | `0` |
| `const k: any = "subarray"; typeof u8[k]` | `function` | `number` |
| `u8.tag = {…}; const k: any = "tag"; u8[k]` | `{"kind":"buffer"}` | `0` |

So no heap value reaches such a local **today**, which is why this classification
bug has never bitten: it is a latent-soundness fix that becomes load-bearing the
moment the behavioural one is repaired. Filed separately rather than folded in —
the numeric-context collectors decide the `buf[i]` i32 fast path, which is the
hottest buffer code in the compiler, and changing them is a measured change.

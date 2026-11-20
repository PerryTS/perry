### Performance

- Loop-version stable counted iteration over packed `Array` and `Array` subclasses, with fallback-free direct reads and mutation-safe current-index side exits.

### Fixed

- The `x | 0` canonical-ToInt32 store lever no longer re-evaluates its operand
  tree as native i32. `expr_produces_canonical_raw_f64` vouches for the RESULT
  of `x | 0`, not for `x`'s operands, so consuming it as a licence to call
  `lower_expr_native(_, I32)` violated that path's documented precondition
  (`can_lower_expr_as_i32_in_current_region`). The i32-chain arm `fptosi`s
  every operand it cannot lower natively, which turned
  `h ^ recv.charCodeAt(i)` on an `any` receiver into an inline `xor i32` over
  whatever that method returned — a BigInt silently produced garbage instead
  of the spec's `TypeError`. The lever now lowers through `lower_expr`, which
  applies `is_provably_not_bigint` per operand, and takes one trailing
  `toint32_fast` to feed the i32 slot; `x | 0` always lowers to `sitofp i32`,
  so that pair folds away. A proven tree still gets the inline `xor i32` and
  keeps the i32-slot store. Pinned by the pre-existing negative control
  `char_code_at_on_an_unproven_receiver_keeps_the_runtime_lowering`.

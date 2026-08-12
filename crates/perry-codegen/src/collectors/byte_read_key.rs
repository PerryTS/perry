//! #7700: ONE answer to "is this `Uint8ArrayGet` a byte read?".
//!
//! `lower/expr_member/member_tail.rs` folds every non-STRING key on a
//! `Uint8Array`/`Buffer`-typed local onto `Expr::Uint8ArrayGet`. A symbol key
//! is not a string, and neither is a `LocalGet` of an `any`-typed local that
//! happens to hold a string at runtime — so the node is NOT a byte read by
//! construction, and its value is whatever the property lookup finds:
//!
//! ```ts
//! const u8 = new Uint8Array([1, 2, 3, 4]);
//! const it = u8[Symbol.iterator];   // a function
//! const k: any = "byteLength";
//! const n = u8[k];                  // 4
//! ```
//!
//! Codegen already knows this. `arrays_finds::lower_uint8array_get_i32` routes
//! an unproven key to `js_object_get_index_polymorphic`, which dispatches
//! numeric keys to the byte read and everything else to the property path, and
//! hands back a boxed JS value. What broke is one level up: the collectors that
//! decide a LOCAL's representation answered "a `Uint8ArrayGet` is a number"
//! unconditionally, so `const it = u8[Symbol.iterator]` got an i32 slot, and
//! `i32_from_indexed_get_lowered` then applied `ToInt32(ToNumber(fn))` to the
//! polymorphic result. `typeof it` was `number`.
//!
//! The correctness condition is about the RUNTIME key, not about which codegen
//! path is taken: when the key really is a number the polymorphic helper
//! returns the byte, and coercing a byte to i32 is exact. So this predicate
//! asks only "is this key a number at runtime?" and is free to admit a local
//! the caller has proven integer-valued even though `is_numeric_expr` would
//! not — the byte read that follows is a number either way.
//!
//! It is an ALLOWLIST on purpose. The blocklist it replaces ("not a string
//! literal") is what shipped this bug: every key kind nobody thought of —
//! symbols first — landed on the wrong side of it.
//!
//! `collectors/pointer_locals.rs` asks a strictly harder question (a local with
//! no shadow slot must not be able to hold a heap value at all), so it passes
//! no local evidence and gets the purely structural answer.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, Param, Stmt, UnaryOp};

/// Every local whose DECLARED type says it holds a number: params, module
/// bindings, and body `let`s at any nesting depth (including a `for`-init
/// counter, which is the shape the byte-read fast path lives in).
///
/// The collectors run before any `FnCtx` exists, and the `binding_types` map
/// they are handed covers only params and module globals — so without this
/// walk a `for (let i = 0; …) sum += buf[i]` counter is untypeable, the key
/// reads as unproven, and the loop loses its i32 representation. That is a
/// measured deoptimization (`js_uint8array_get` → `js_uint8array_index_get_value`,
/// i32 slot → double slot), not a theoretical one.
///
/// Declared types are evidence here in a way they are NOT generally: Perry
/// does not enforce annotations, but a mis-annotated key is a pre-existing
/// wrong-code hazard on the byte path, identical to the one before #7700.
pub(crate) fn collect_numeric_typed_locals(
    stmts: &[Stmt],
    params: &[Param],
    binding_types: &HashMap<u32, HirType>,
) -> HashSet<u32> {
    fn is_numeric(ty: &HirType) -> bool {
        matches!(ty, HirType::Number | HirType::Int32)
    }
    let mut out: HashSet<u32> = binding_types
        .iter()
        .filter(|(_, ty)| is_numeric(ty))
        .map(|(id, _)| *id)
        .collect();
    for p in params {
        if is_numeric(&p.ty) {
            out.insert(p.id);
        }
    }
    walk(stmts, &mut out);
    return out;

    fn walk(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for s in stmts {
            match s {
                Stmt::Let { id, ty, .. } => {
                    if is_numeric(ty) {
                        out.insert(*id);
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(eb) = else_branch {
                        walk(eb, out);
                    }
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        walk(std::slice::from_ref(init_stmt), out);
                    }
                    walk(body, out);
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out);
                    if let Some(c) = catch {
                        walk(&c.body, out);
                    }
                    if let Some(f) = finally {
                        walk(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        walk(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => walk(std::slice::from_ref(body.as_ref()), out),
                _ => {}
            }
        }
    }
}

/// Is `index` a number at runtime, so `u8[index]` provably reads a byte?
///
/// `is_numeric_local` supplies the caller's evidence for a bare local — a
/// declared-type lookup, an integer-candidate set, or `|_| false` for a caller
/// that has none. Every other arm is structural, so the answer cannot drift
/// between the collectors that share it.
pub(crate) fn uint8array_get_reads_a_byte(
    index: &Expr,
    is_numeric_local: &mut dyn FnMut(u32) -> bool,
) -> bool {
    match index {
        Expr::Integer(_) | Expr::Number(_) => true,
        // A byte read / a typed-array length is itself a number, so it is a
        // number-valued key.
        Expr::Uint8ArrayGet { index, .. } => uint8array_get_reads_a_byte(index, is_numeric_local),
        Expr::BufferIndexGet { .. } | Expr::Uint8ArrayLength(_) | Expr::BufferLength(_) => true,
        // Explicit ToNumber.
        Expr::NumberCoerce(_) => true,
        // `Math.*` coerce their operands internally (ToNumber — BigInt and
        // Symbol throw) and return a raw double. Only the arms that plausibly
        // appear in an index position are listed; an omission costs a
        // representation, never correctness.
        Expr::MathFloor(..)
        | Expr::MathCeil(..)
        | Expr::MathRound(..)
        | Expr::MathTrunc(..)
        | Expr::MathAbs(..)
        | Expr::MathMin(..)
        | Expr::MathMax(..)
        | Expr::MathImul(..) => true,
        // `-x` / `+x` / `~x` are ToNumber/ToInt32 — except on a BigInt, which
        // stays a BigInt. Requiring a numeric operand excludes that case; it
        // is stricter than `is_numeric_expr`'s `!is_bigint_expr` test, which
        // is the safe direction here.
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot)
                && uint8array_get_reads_a_byte(operand, is_numeric_local)
        }
        Expr::Binary { op, left, right } => match op {
            // ToInt32/ToUint32 — a number regardless of operand shape.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => true,
            // Arithmetic is ToNumeric on both sides: a number unless an
            // operand is a BigInt, in which case the result is a BigInt.
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                uint8array_get_reads_a_byte(left, is_numeric_local)
                    && uint8array_get_reads_a_byte(right, is_numeric_local)
            }
            // `+` may be string concatenation.
            BinaryOp::Add => {
                uint8array_get_reads_a_byte(left, is_numeric_local)
                    && uint8array_get_reads_a_byte(right, is_numeric_local)
            }
        },
        // `x++` / `--x` evaluate to `ToNumeric(x) ± 1` — a number unless `x`
        // is a BigInt, which the caller's evidence has to rule out.
        Expr::LocalGet(id) | Expr::Update { id, .. } => is_numeric_local(*id),
        // Notably NOT here: `SymbolFor` (`u8[Symbol.iterator]`), `String`,
        // template literals, calls, and property reads.
        _ => false,
    }
}

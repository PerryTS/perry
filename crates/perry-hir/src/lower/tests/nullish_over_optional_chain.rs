//! `??` over an optional chain must not take its right operand's type
//! (#9610-adjacent typing rule). Split from `tests.rs` for the 2000-line cap.

use super::*;

/// `const masks = opts?.masks ?? null` must not be declared `Null`. The
/// AST-level `??` rule used to answer the right operand's type whenever the
/// left inferred `Any` — and an optional chain always does — so the binding
/// was typed `Null`, which downstream read as "holds no pointer".
#[test]
fn nullish_coalescing_over_an_optional_chain_is_not_typed_by_its_right_operand() {
    let source = r#"
        function add(opts: { masks: number[] } | null) {
            const masks = opts?.masks ?? null;
            return masks;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "coalesce.ts").expect("source parses");
    let hir = super::lower_module(&module, "coalesce", "coalesce.ts").expect("source lowers");
    let add = hir
        .functions
        .iter()
        .find(|f| f.name == "add")
        .expect("`add` lowers");
    let masks_ty = add
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { name, ty, .. } if name == "masks" => Some(ty),
            _ => None,
        })
        .expect("`masks` lowers to a Let");
    assert!(
        !matches!(masks_ty, Type::Null | Type::Void),
        "`opts?.masks ?? null` is an array on the non-null path; got {masks_ty:?}"
    );
}

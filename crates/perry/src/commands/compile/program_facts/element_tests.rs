//! Product-mode element identity cannot introduce verdict sites or elisions.
use super::*;
use perry_hir::{Expr, Stmt};

fn clear_element_identity(expr: &mut Expr) {
    match expr {
        Expr::IndexGet { element_offset, .. }
        | Expr::IndexSet { element_offset, .. }
        | Expr::IndexUpdate { element_offset, .. }
        | Expr::PutValueSet { element_offset, .. } => *element_offset = 0,
        _ => {}
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut clear_element_identity);
}

#[test]
fn product_element_identity_preserves_codegen_facts() {
    let source = r#"const recv: any = { value: 1 };
        const key: any = "value";
        recv[key]; recv[key] = 2; recv[key](); recv[key]++;
        recv.value;"#;
    let ast = perry_parser::parse_typescript(source, "product.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "product", "product.ts").unwrap();
    let mut before = hir.clone();
    for stmt in &mut before.init {
        match stmt {
            Stmt::Expr(expr) => clear_element_identity(expr),
            Stmt::Let {
                init: Some(expr), ..
            } => clear_element_identity(expr),
            _ => {}
        }
    }
    assert_eq!(
        perry_hir::stable_hash::hash_module(&hir),
        perry_hir::stable_hash::hash_module(&before)
    );
    let cfg = perry_wpfacts::FactsConfig::default();
    let old = perry_wpfacts::compute(&[("product.ts".into(), &before)], &cfg);
    let new = perry_wpfacts::compute(&[("product.ts".into(), &hir)], &cfg);
    assert!(new.elements.iter().any(|e| e.byte_offset != 0));
    assert!(old.elements.iter().all(|e| e.byte_offset == 0));
    assert_eq!(format!("{:?}", old.sites), format!("{:?}", new.sites));
    assert_eq!(old.stats.work, new.stats.work);
    assert_eq!(old.stats.pops, new.stats.pops);
    assert_eq!(old.stats.constraints, new.stats.constraints);
    // `number_element_reads` and `primitive_elements` are keyed by
    // `element_offset`, and offset 0 means "no identity", so without identity
    // they cannot be stated at all. Identity may add those two; every other
    // elision must be unchanged.
    let old_elision = elision_for(&old, "product.ts");
    let mut new_elision = elision_for(&new, "product.ts");
    assert!(old_elision.number_element_reads.is_empty());
    assert!(old_elision.primitive_elements.is_empty());
    new_elision.number_element_reads.clear();
    new_elision.primitive_elements.clear();
    assert_eq!(old_elision, new_elision);
    assert_eq!(old.digest_for("product.ts"), new.digest_for("product.ts"));
}

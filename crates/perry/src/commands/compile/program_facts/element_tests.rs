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
    // The facts keyed by `element_offset` exist only with element identity:
    // an element site with offset 0 has no key (P3 B1 59925ca54d
    // `number_element_reads`, #14537 91a26fd9c9 `primitive_elements`, ...).
    // Without identity each is empty; with it, each key is an element site
    // of the program. Every other codegen fact is equal.
    let mut sites = std::collections::BTreeSet::new();
    let mut with_identity = hir.clone();
    for stmt in &mut with_identity.init {
        if let Stmt::Expr(expr)
        | Stmt::Let {
            init: Some(expr), ..
        } = stmt
        {
            element_offsets(expr, &mut sites);
        }
    }
    let (old_elision, old_keys) = take_element_keyed(elision_for(&old, "product.ts"));
    let (new_elision, new_keys) = take_element_keyed(elision_for(&new, "product.ts"));
    assert!(
        old_keys.is_empty(),
        "element facts without element identity: {old_keys:?}"
    );
    assert!(
        new_keys.iter().all(|k| sites.contains(k)),
        "element facts {new_keys:?} keyed by no element site of {sites:?}"
    );
    assert_eq!(old_elision, new_elision);
    assert_eq!(old.digest_for("product.ts"), new.digest_for("product.ts"));
}

fn element_offsets(expr: &mut Expr, out: &mut std::collections::BTreeSet<u32>) {
    match expr {
        Expr::IndexGet { element_offset, .. }
        | Expr::IndexSet { element_offset, .. }
        | Expr::IndexUpdate { element_offset, .. }
        | Expr::PutValueSet { element_offset, .. }
            if *element_offset != 0 =>
        {
            out.insert(*element_offset);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |e| element_offsets(e, out));
}

/// The elision facts with every map keyed by `element_offset` emptied, and
/// the keys those maps held.
fn take_element_keyed(
    mut e: perry_codegen::ElisionFacts,
) -> (perry_codegen::ElisionFacts, std::collections::BTreeSet<u32>) {
    use std::mem::take;
    let mut keys = std::collections::BTreeSet::new();
    keys.extend(take(&mut e.element_reads).into_keys());
    keys.extend(take(&mut e.number_element_reads).into_keys());
    keys.extend(take(&mut e.int32_element_reads));
    keys.extend(take(&mut e.element_floors).into_keys());
    keys.extend(take(&mut e.element_keys).into_keys());
    keys.extend(take(&mut e.key_signs).into_keys());
    keys.extend(take(&mut e.wide_keys).into_keys());
    keys.extend(take(&mut e.inbounds_stores));
    keys.extend(take(&mut e.inbounds_reads).into_keys());
    keys.extend(take(&mut e.rest_reads).into_keys());
    keys.extend(take(&mut e.rest_writes).into_keys());
    keys.extend(take(&mut e.quiet_elements));
    keys.extend(take(&mut e.primitive_elements));
    (e, keys)
}

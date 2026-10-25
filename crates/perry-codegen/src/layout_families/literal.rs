//! Closed literal families keep their source-ordered base slots. A bounded
//! late suffix is optional; it never changes the literal's prototype.
use super::*;

pub(super) fn own_keys(class: &Class) -> Result<OwnKeys, String> {
    let mut keys = Vec::with_capacity(class.fields.len());
    for field in &class.fields {
        if !crate::layout_desc::key_is_atom(&field.name)
            || field.name == "__proto__"
            || field.name == "constructor"
            || keys.iter().any(|(key, _)| key == &field.name)
        {
            return Err("literal key outside a closed atom layout".into());
        }
        // is_literal_shape checked the exact strict positional constructor.
        // Its argument values have already been evaluated before allocation.
        keys.push((FamilyKey::Name(field.name.clone()), true));
    }
    Ok(OwnKeys {
        keys,
        super_inert: true,
    })
}

pub(super) fn retain_late(
    raw: &mut Vec<RawFamily>,
    owns: &mut Vec<usize>,
    literals: &BTreeSet<u32>,
) {
    let mut at = 0;
    raw.retain(|family| {
        let keep = !literals.contains(&family.class_id) || family.keys.len() > owns[at];
        if keep {
            at += 1;
        } else {
            owns.remove(at);
        }
        keep
    });
}

pub(super) fn finish(layouts: &mut ProgramLayouts, literals: &BTreeSet<u32>) {
    for (id, family) in &mut layouts.families {
        if !literals.contains(id) {
            continue;
        }
        layouts.literal_families.insert(*id);
        // Literal base stores already name source-order slots. Keep the
        // prefix and place late holes after it, independent of key colours.
        for (slot, (_, at)) in family.keys.iter_mut().enumerate() {
            *at = slot as u32;
        }
        family.slot_count = family.keys.len() as u32;
        debug_assert!(family.variants.is_empty());
    }
    if layouts.literal_families.is_empty() {
        return;
    }
    layouts.coloured_family_keys = 0;
    layouts.coloured_placed = 0;
    for family in layouts.families.values() {
        for (key, at) in &family.keys {
            if let Some((_, colour)) = key.name().and_then(|key| layouts.colored.get(key)) {
                layouts.coloured_family_keys += 1;
                layouts.coloured_placed += u32::from(colour == at);
            }
        }
    }
    use std::hash::{Hash, Hasher};
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    for family in layouts.families.values() {
        family.hash(&mut digest);
    }
    layouts.colored.hash(&mut digest);
    layouts.literal_families.hash(&mut digest);
    layouts.digest = digest.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (HirModule, ProgramLayouts, u32) {
        let source = r#"
            function make(value) {
                const record = { first: value, second: value, next: null };
                return record;
            }
            function extend(value) {
                const record = make(value);
                record.later = value;
                return record;
            }
            function walk(node) {
                let total = 0;
                while (node) {
                    total += node.first + node.second;
                    node = node.next;
                }
                return total;
            }
            const item = extend(3);
            item.extra = 4;
            walk(item);
        "#;
        let ast = perry_parser::parse_typescript(source, "literal-families.ts").unwrap();
        let hir = perry_hir::lower_module(&ast, "literal-families", "literal-families.ts").unwrap();
        let id = hir
            .classes
            .iter()
            .find(|c| c.is_literal_shape() && c.fields.iter().any(|f| f.name == "first"))
            .expect("closed base literal must be live")
            .id;
        let layouts = compute([("literal-families.ts", &hir)], true, &BTreeMap::new());
        (hir, layouts, id)
    }

    #[test]
    fn literal_late_family_keeps_base_slots_and_holes() {
        let (_, layouts, id) = fixture();
        assert!(
            layouts.literal_families.contains(&id),
            "late literal must be admitted"
        );
        let family = &layouts.families[&id];
        assert_eq!(family.own_count, 3);
        assert_eq!(family.init_count, 3);
        assert!(
            family.keys.iter().any(|(k, _)| k == "later"),
            "factory late store must be included"
        );
        assert!(
            family.keys.iter().any(|(k, _)| k == "extra"),
            "caller late store must be included"
        );
        assert!(family.slot_count > family.own_count);
        for (slot, key) in ["first", "second", "next"].iter().enumerate() {
            assert!(family
                .keys
                .iter()
                .any(|(k, at)| k == *key && *at == slot as u32));
        }
        assert!(
            family.variants.is_empty(),
            "factory positions are not constructor positions"
        );
    }

    #[test]
    fn reassigned_receiver_gets_a_candidate_not_a_lifetime_proof() {
        let (hir, layouts, id) = fixture();
        let walk = hir.functions.iter().find(|f| f.name == "walk").unwrap();
        let node = walk.params[0].id;
        let candidates = receiver_candidates(&hir, &layouts);
        assert_eq!(
            candidates.get(&node),
            Some(&id),
            "loop receiver must have a candidate"
        );
        let mut reassigned = false;
        visit_stmts(&walk.body, &mut |_| {}, &mut |expr| {
            if matches!(expr, Expr::LocalSet(local, _) if *local == node) {
                reassigned = true;
            }
        });
        assert!(reassigned, "the fixture must exercise reassignment");
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            program_layouts: std::sync::Arc::new(layouts),
            ..Default::default()
        };
        let ir = String::from_utf8(crate::compile_module(&hir, opts).unwrap()).unwrap();
        let walk_ir = ir
            .split("\ndefine ")
            .find(|body| {
                body.lines()
                    .next()
                    .is_some_and(|line| line.contains("walk("))
            })
            .expect("walk function must be emitted");
        let walk_ir = walk_ir.split("\n}").next().unwrap();
        assert_eq!(
            walk_ir
                .lines()
                .filter(|line| line.contains("class_family.word") && line.ends_with(':'))
                .count(),
            3,
            "each of the loop's three reads needs its current-shape guard"
        );
        assert_eq!(
            walk_ir
                .lines()
                .filter(|line| line.contains("class_family.present") && line.ends_with(':'))
                .count(),
            3,
            "each read needs its own current-slot presence test"
        );
        assert!(
            ir.contains(&format!("@perry_layout_class_{id}")),
            "literal descriptor must be live"
        );
        let descriptor = ir
            .lines()
            .find(|line| line.starts_with(&format!("@perry_layout_class_{id} = global")))
            .expect("literal family descriptor must be defined");
        assert!(
            descriptor.contains("i32 -1, i64 0, i64 0,"),
            "literal family must register a carried layout with Object.prototype"
        );
        assert!(
            descriptor.contains("i32 0, i32 0, ptr null"),
            "literal identity must not claim a synthetic class prototype"
        );
        assert!(
            walk_ir.contains("icmp eq i32"),
            "current shape must be compared"
        );
    }

    #[test]
    fn literal_without_late_stores_keeps_its_existing_birth() {
        let ast = perry_parser::parse_typescript(
            "const value = { first: 1, second: 2 }; console.log(value.first);",
            "closed.ts",
        )
        .unwrap();
        let hir = perry_hir::lower_module(&ast, "closed", "closed.ts").unwrap();
        assert!(hir.classes.iter().any(Class::is_literal_shape));
        let layouts = compute([("closed.ts", &hir)], true, &BTreeMap::new());
        assert!(layouts.literal_families.is_empty());
        assert!(receiver_candidates(&hir, &layouts).is_empty());
    }
}

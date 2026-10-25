//! Runtime import bindings outrank implicit bare-name class metadata.
//! The latter supports inferred producer class types at member dispatch sites;
//! it does not establish a lexical binding in the consuming source.
use perry_hir::{Import, ImportSpecifier};
use std::collections::HashSet;

pub(super) fn explicit_import_names(imports: &[Import]) -> HashSet<String> {
    imports
        .iter()
        .filter(|import| !import.type_only && !import.runtime_erased)
        .flat_map(|import| &import.specifiers)
        .map(|specifier| match specifier {
            ImportSpecifier::Named { local, .. }
            | ImportSpecifier::Default { local }
            | ImportSpecifier::Namespace { local } => local.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(source: &str) -> HashSet<String> {
        let ast = perry_parser::parse_typescript(source, "bindings.ts").unwrap();
        let module = perry_hir::lower_module(&ast, "bindings", "bindings.ts").unwrap();
        explicit_import_names(&module.imports)
    }

    #[test]
    fn runtime_import_bindings_use_local_aliases_and_exclude_erased_imports() {
        let names = bindings(
            r#"
            import { Service as Current } from "./a.ts";
            import Default from "./b.ts";
            import * as Namespace from "./c.ts";
            import type { TypeOnly } from "./d.ts";
            import { type Erased } from "./e.ts";
            "#,
        );
        assert_eq!(
            names,
            HashSet::from(["Current".into(), "Default".into(), "Namespace".into()])
        );
        // The producer's bare Service metadata remains usable for inferred
        // instance types, while Current is the actual lexical value binding.
        assert!(!names.contains("Service"));
    }

    #[test]
    fn all_import_orders_claim_the_same_bindings_before_class_metadata() {
        let imports = [
            r#"import { helper } from "./a.ts";"#,
            r#"import { A } from "./a.ts";"#,
            r#"import { B } from "./b.ts";"#,
            r#"import { Service } from "./b.ts";"#,
        ];
        let expected = HashSet::from(["helper".into(), "A".into(), "B".into(), "Service".into()]);
        let mut orders = 0;
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let order = [a, b, c, d];
                        if order.iter().copied().collect::<HashSet<_>>().len() != 4 {
                            continue;
                        }
                        let source = order.map(|i| imports[i]).join("\n");
                        assert_eq!(bindings(&source), expected, "{order:?}");
                        orders += 1;
                    }
                }
            }
        }
        assert_eq!(orders, 24);
    }
}

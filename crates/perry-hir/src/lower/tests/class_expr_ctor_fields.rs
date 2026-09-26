//! #10499: a class EXPRESSION's top-level constructor `this.<name> = …`
//! assignments must be inferred as own fields exactly like the identical
//! class DECLARATION's. Pre-fix only `lower_class_decl` ran the scan, so
//! `var NodeExpr = class {…}` lowered with `fields: 0` and every constructor
//! store was a generic `[[Set]]` property add (159× slower than Node).

fn lower(source: &str) -> crate::ir::Module {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&module, "t", "t.ts").expect("source lowers")
}

fn field_names(hir: &crate::ir::Module, class: &str) -> Vec<String> {
    hir.classes
        .iter()
        // A NAMED class expression registers as `<name>__class_expr_<n>`.
        .find(|c| c.name == class || c.name.starts_with(&format!("{class}__class_expr_")))
        .unwrap_or_else(|| {
            panic!(
                "fixture declares class {class}; classes: {:?}",
                hir.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
            )
        })
        .fields
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

/// The issue's reproducer: the expression and the declaration get the same
/// six fields, in the same (execution) order.
#[test]
fn class_expression_infers_ctor_fields_like_declaration() {
    let hir = lower(
        r#"
        class NodeDecl { constructor(kind, pos, end) { this.pos = pos; this.end = end; this.kind = kind; this.id = 0; this.flags = 0; this.parent = undefined; } }
        var NodeExpr = class { constructor(kind, pos, end) { this.pos = pos; this.end = end; this.kind = kind; this.id = 0; this.flags = 0; this.parent = undefined; } };
        "#,
    );
    let expected = ["pos", "end", "kind", "id", "flags", "parent"];
    assert_eq!(field_names(&hir, "NodeDecl"), expected);
    assert_eq!(field_names(&hir, "NodeExpr"), expected);
}

/// The declaration's exclusions apply to expressions too: own methods
/// (self-binding) and accessors are not data fields; declared fields are not
/// duplicated; minified comma sequences are scanned.
#[test]
fn class_expression_keeps_declaration_exclusions() {
    let hir = lower(
        r#"
        var C = class {
            declared = 1;
            constructor(p) {
                this.declared = 2;
                this.run = this.run.bind(this);
                this.points = p;
                (this.a = 1), (this.b = 2);
            }
            run() {}
            set points(v) {}
            get points() { return 0; }
        };
        "#,
    );
    assert_eq!(field_names(&hir, "C"), ["declared", "a", "b"]);
}

/// A declaration subclass of a class-expression base must see the base's
/// inferred fields as inherited and not re-add them as own fields (two slots
/// for one name).
#[test]
fn declaration_subclass_excludes_class_expression_parent_fields() {
    let hir = lower(
        r#"
        var Base = class { constructor() { this.kind = "base"; this.shared = 1; } };
        class DeclSub extends Base { constructor() { super(); this.shared = 3; this.tag = 4; } }
        "#,
    );
    assert_eq!(field_names(&hir, "Base"), ["kind", "shared"]);
    assert_eq!(field_names(&hir, "DeclSub"), ["tag"]);
}

/// A class expression whose parent is only known at runtime (a mixin
/// parameter, `extends pick()`) must NOT infer fields: an own slot for a
/// name the runtime parent's constructor also writes would shadow the
/// parent's value.
#[test]
fn class_expression_over_runtime_parent_infers_no_fields() {
    let hir = lower(
        r#"
        class Left { constructor() { this.side = "left"; } }
        function pick() { return Left; }
        var Picked = class extends pick() { constructor() { super(); this.side = this.side + "!"; this.own = 1; } };
        function Tagged(BaseClass) {
            return class TaggedImpl extends BaseClass { constructor() { super(); this.kind = 1; } };
        }
        "#,
    );
    assert!(field_names(&hir, "Picked").is_empty());
    assert!(field_names(&hir, "TaggedImpl").is_empty());
}

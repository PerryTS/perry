//! #10487: a subclass constructor assignment `this.m = …` must NOT become an
//! own inline field slot when `m` is a method INHERITED from a parent class
//! — that shadowed the inherited method with an own `undefined` field from
//! the moment `super()` returned, until the assignment statement ran. Split
//! from `tests.rs` for the 2000-line file cap.

/// `this.close = …` in a subclass constructor, where `close` is a method
/// declared only on the parent, must not allocate an own `close` field.
#[test]
fn subclass_ctor_assignment_to_inherited_method_name_is_not_a_field() {
    let source = r#"
        class Base {
            close() { return "closed"; }
        }
        class Sub extends Base {
            constructor() {
                super();
                this.seen = typeof this.close;
                this.close = () => "replaced";
            }
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let sub = hir
        .classes
        .iter()
        .find(|c| c.name == "Sub")
        .expect("fixture declares class Sub");
    assert!(
        !sub.fields.iter().any(|f| f.name == "close"),
        "this.close = … must not become an own field on Sub when `close` is \
         inherited from Base; fields: {:?}",
        sub.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert!(
        sub.fields.iter().any(|f| f.name == "seen"),
        "this.seen = … has no parent-declared counterpart and must still \
         become an own field; fields: {:?}",
        sub.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// Same requirement across TWO levels of inheritance (the method is
/// declared on a grandparent, not the immediate parent).
#[test]
fn grandparent_method_name_is_excluded_across_two_levels() {
    let source = r#"
        class Base {
            close() { return "closed"; }
        }
        class Mid extends Base {}
        class Grand extends Mid {
            constructor() {
                super();
                this.close = () => "replaced";
            }
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let grand = hir
        .classes
        .iter()
        .find(|c| c.name == "Grand")
        .expect("fixture declares class Grand");
    assert!(
        !grand.fields.iter().any(|f| f.name == "close"),
        "this.close = … must not become an own field on Grand when `close` \
         is inherited from Base via Mid; fields: {:?}",
        grand.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// Control: a class's OWN method being self-bound in its OWN constructor
/// (the pre-existing #665-adjacent zod fix) must keep working — this
/// exclusion is orthogonal to the inherited-method one added here.
#[test]
fn own_class_method_self_assignment_is_still_not_a_field() {
    let source = r#"
        class Own {
            close() { return "closed"; }
            constructor() {
                this.close = this.close.bind(this);
            }
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let own = hir
        .classes
        .iter()
        .find(|c| c.name == "Own")
        .expect("fixture declares class Own");
    assert!(
        !own.fields.iter().any(|f| f.name == "close"),
        "an own-method self-assignment must not become a field either; \
         fields: {:?}",
        own.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

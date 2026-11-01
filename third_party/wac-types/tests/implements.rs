use wac_types::{ItemKind, Package, Types};

/// A component that imports the same interface twice under two different
/// plain-name labels using the component model `(implements "I")` directive.
const IMPLEMENTS_IMPORTS: &str = r#"(component
    (type (;0;)
        (instance
            (type (;0;) (func))
            (export (;0;) "do-something" (func (type 0)))
        )
    )
    (import "primary" (implements "test:test/iface") (instance (;0;) (type 0)))
    (import "backup" (implements "test:test/iface") (instance (;1;) (type 0)))
)"#;

/// Parsing a component with `(implements "I")` imports should preserve both
/// labeled imports and resolve their interface id to the implemented interface
/// name rather than the opaque label.
#[test]
fn implements_import_interface_id() {
    let bytes = wat::parse_str(IMPLEMENTS_IMPORTS).expect("failed to parse WAT");
    let mut types = Types::default();
    let package = Package::from_bytes("test:consumer", None, bytes, &mut types)
        .expect("failed to parse component");

    let world = &types[package.ty()];

    // Both labeled imports are present, keyed by their plain-name label.
    assert!(
        world.imports.contains_key("primary"),
        "expected `primary` implements import"
    );
    assert!(
        world.imports.contains_key("backup"),
        "expected `backup` implements import"
    );

    // Each import is an instance whose interface id is the implemented
    // interface, not the label.
    for name in ["primary", "backup"] {
        let ItemKind::Instance(id) = world.imports[name] else {
            panic!("expected instance kind for import `{name}`");
        };
        assert_eq!(
            types[id].id.as_deref(),
            Some("test:test/iface"),
            "implements import `{name}` should resolve to interface `test:test/iface`"
        );
    }
}

/// A regular interface import and an `(implements "I")` import of the same
/// interface should coexist, both resolving to the same interface id.
#[test]
fn implements_with_regular_import() {
    let wat = r#"(component
        (type (;0;)
            (instance
                (type (;0;) (func))
                (export (;0;) "do-something" (func (type 0)))
            )
        )
        (import "test:test/iface" (instance (;0;) (type 0)))
        (import "secondary" (implements "test:test/iface") (instance (;1;) (type 0)))
    )"#;

    let bytes = wat::parse_str(wat).expect("failed to parse WAT");
    let mut types = Types::default();
    let package = Package::from_bytes("test:consumer", None, bytes, &mut types)
        .expect("failed to parse component");

    let world = &types[package.ty()];

    for name in ["test:test/iface", "secondary"] {
        let ItemKind::Instance(id) = *world
            .imports
            .get(name)
            .unwrap_or_else(|| panic!("expected import `{name}` to be present"))
        else {
            panic!("expected instance kind for import `{name}`");
        };
        assert_eq!(types[id].id.as_deref(), Some("test:test/iface"));
    }
}

use crate::{Host, Imports, Options};

#[test]
fn distinct_wit_resources_cannot_alias_one_guest_resource() {
    let world=perry_wit::parse("package test:nominal; interface api { resource first; resource second; } world plugin { export api; }",None).unwrap();
    let component = wat::parse_str(
        r#"(component
      (type $item (resource (rep i32)))
      (instance $api (export "first" (type $item)) (export "second" (type $item)))
      (export "test:nominal/api" (instance $api)))"#,
    )
    .unwrap();
    let error = Host::new(&component, &world, &Imports::new(), &Options::default())
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("distinct WIT resources share a canonical identity"));
}

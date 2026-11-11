use super::*;
use perry_hir::realm_manifest::{RealmEntry, RealmKind, RealmOwn, RealmOwnValue};
use perry_hir::realm_reach_index::RealmIndex;

#[test]
fn holders_select_exactly_the_logical_members() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for source in [
        "console.log('hi');",
        "const B: any = Buffer; console.log(typeof B.alloc, typeof B.prototype.readUInt8);",
        "console.log(Object.getOwnPropertyNames(Buffer.prototype).length, Object.keys(Error).length);",
        "console.log(Buffer); console.log(TypeError);",
        "for (const k in Promise) console.log(k); console.log(JSON.stringify(Object.getOwnPropertyDescriptors(Set)));",
        "function f(o,k) { o[k]; } Buffer;",
        "'x'.constructor.fromCharCode(65);",
        "const b = Buffer.alloc(2); console.log(b.readUInt8(0), new URL('http://a/b').pathname, [1].map(x => x).length, new Set([1]).size);",
    ] {
        let module = perry_parser::parse_typescript(source, "selection.ts").unwrap();
        let reach = perry_hir::realm_reach::scan(&module, &index);
        assert!(!reach.is_full(), "{source}");
        let plan = RealmPlan::from_reach(&reach).unwrap();
        let mut omitted = 0;
        for (name, cell) in &plan.cells {
            let row = index.last(name).unwrap();
            let original: RealmDeclaration = serde_json::from_value(row.facts().clone()).unwrap();
            if row.runtime_layout_holder() {
                assert_eq!(cell.properties.len(), original.properties.len(), "{name}");
                continue;
            }
            for property in &original.properties {
                let enumerable = property.attributes & 2 != 0;
                let get = property.get.as_ref().filter(|n| reach.keeps_member(n, enumerable));
                let set = property.set.as_ref().filter(|n| reach.keeps_member(n, enumerable));
                let value = match &property.value {
                    Some(RealmValue::Reference(n) | RealmValue::RawReference(n)) => n == "%Global%" || reach.keeps_member(n, enumerable),
                    Some(_) => true,
                    None => false,
                };
                let selected = cell.properties.iter().find(|p| p.key == property.key && p.symbol == property.symbol);
                assert_eq!(selected.is_some(), value || get.is_some() || set.is_some(), "{source}: {name}.{}", property.key);
                if let Some(selected) = selected {
                    assert_eq!(selected.get.as_ref(), get);
                    assert_eq!(selected.set.as_ref(), set);
                } else {
                    omitted += 1;
                }
            }
        }
        assert!(omitted > 0, "selection was not exercised: {source}");
    }
}

#[test]
fn f7_plan_keeps_the_bind_time_constructor_prototype() {
    const OWN: &[RealmOwn] = &[RealmOwn {
        key: "prototype",
        symbol: false,
        value: RealmOwnValue::Reference("Ctor.prototype"),
    }];
    let index = RealmIndex::new(&[
        RealmEntry {
            name: "Ctor",
            kind: RealmKind::Constructor,
            births: &["Ctor.prototype"],
            module: None,
            facts: r#"{"constructor":{}}"#,
            effects: &[],
            own: OWN,
            proto: None,
        },
        RealmEntry {
            name: "Ctor.prototype",
            kind: RealmKind::Prototype,
            births: &[],
            module: None,
            facts: "{}",
            effects: &[],
            own: &[],
            proto: None,
        },
    ]);
    let mut reach = RealmReach::default();
    reach.own("Ctor");
    reach.close(&index);
    let declarations = [
        r#"{"name":"%Global%","kind":"namespace","header":{"gc_type":2,"reserved":0}}"#,
        r#"{"name":"Ctor","kind":"constructor","header":{"gc_type":2,"reserved":0},"properties":[{"key":"prototype","symbol":false,"attributes":0,"value":{"kind":"reference","value":"Ctor.prototype"}}],"own_key_order":["prototype"]}"#,
        r#"{"name":"Ctor.prototype","kind":"prototype","header":{"gc_type":2,"reserved":0}}"#,
    ].iter().map(|text| serde_json::from_str(text).unwrap()).collect();
    let plan = RealmPlan::from_declarations(&reach, declarations, &index).unwrap();
    assert!(reach.whole.contains("Ctor.prototype"));
    assert_eq!(plan.cells["Ctor"].properties.len(), 1);
    assert_eq!(plan.cells["Ctor"].properties[0].key, "prototype");
    assert!(plan.cells.contains_key("Ctor.prototype"));
}

use super::*;
use crate::realm_manifest::{RealmEntry, RealmKind, RealmOwn, RealmOwnValue};
fn row(
    name: &'static str,
    kind: RealmKind,
    births: &'static [&'static str],
    own: &'static [RealmOwn],
    facts: &'static str,
) -> RealmEntry {
    RealmEntry {
        name,
        kind,
        births,
        module: None,
        facts,
        effects: &[],
        own,
        proto: None,
    }
}
const HOLD: &[RealmOwn] = &[RealmOwn {
    key: "held",
    symbol: false,
    value: RealmOwnValue::Reference("Holder"),
}];
const NEXT: &[RealmOwn] = &[RealmOwn {
    key: "next",
    symbol: false,
    value: RealmOwnValue::Reference("Holder.next"),
}];
const PROTOTYPE: &[RealmOwn] = &[RealmOwn {
    key: "prototype",
    symbol: false,
    value: RealmOwnValue::Reference("Ctor.prototype"),
}];
const KEYS: &[RealmOwn] = &[RealmOwn {
    key: "keys",
    symbol: false,
    value: RealmOwnValue::Reference("Object.keys"),
}];
fn index() -> RealmIndex {
    RealmIndex::new(&[
        row(
            "Object",
            RealmKind::Constructor,
            &["Object.keys"],
            KEYS,
            "{}",
        ),
        row("Object.keys", RealmKind::Method, &[], &[], "{}"),
        row("Owner", RealmKind::Namespace, &["Holder"], HOLD, "{}"),
        row(
            "Holder",
            RealmKind::Prototype,
            &["Holder.next"],
            NEXT,
            r#"{"flags":["runtime_key:next"]}"#,
        ),
        row("Holder.next", RealmKind::Method, &["Body"], &[], "{}"),
        row("Body", RealmKind::Constructor, &[], &[], "{}"),
        row(
            "Ctor",
            RealmKind::Constructor,
            &["Ctor.prototype"],
            PROTOTYPE,
            r#"{"constructor":{}}"#,
        ),
        row("Ctor.prototype", RealmKind::Prototype, &[], &[], "{}"),
    ])
}
#[test]
fn runtime_spelled_members_require_a_whole_holder() {
    let index = index();
    let mut structure = RealmReach::default();
    structure.own("Owner");
    structure.close(&index);
    assert!(structure.structure.contains("Holder"));
    assert!(structure.structure.contains("Holder.next"));
    assert!(!structure.names.contains("Body"));
    let mut whole = RealmReach::default();
    whole.own("Holder");
    whole.close(&index);
    assert!(whole.whole.contains("Holder.next"));
    assert!(whole.names.contains("Body"));
}
#[test]
fn reclose_preserves_the_partition_and_body_births() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.own("Owner");
    reach.close(&index);
    let once = reach.clone();
    reach.close(&index);
    assert_eq!(reach, once);
    reach.receiver_keys.insert("next".into());
    reach.close(&index);
    assert!(reach.whole.contains("Holder.next") && reach.names.contains("Body"));
    let once = reach.clone();
    reach.close(&index);
    assert_eq!(reach, once);
}
#[test]
fn constructor_prototype_is_owned_by_its_bind_read() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.own("Ctor");
    reach.close(&index);
    assert!(reach.whole.contains("Ctor.prototype"));
    assert!(reach.keeps_member("Ctor.prototype", false));
}
#[test]
fn keyless_reads_keep_values_without_following_their_bodies() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.own("Owner");
    reach.close(&index);
    assert!(!reach.keeps_member("Holder.next", true));
    reach.keyless_read = Some(KeylessRead::Enumerable);
    assert!(reach.keeps_member("Holder.next", true));
    assert!(!reach.keeps_member("Holder.next", false));
    reach.keyless_read = Some(KeylessRead::All);
    assert!(reach.keeps_member("Holder.next", false));
    reach.close(&index);
    assert!(!reach.names.contains("Body"));
}
#[test]
fn scan_carries_receiver_setter_and_keyless_facts() {
    let index = index();
    let source = "function f(o,k) { o.next(); o[k]; o.next = 1; Object.keys(o); }";
    let module = perry_parser::parse_typescript(source, "partition.cjs").unwrap();
    let reach = scan(&module, &index);
    assert!(reach.receiver_keys.contains("next"));
    assert!(reach.any_receiver_key);
    assert!(reach.setter_keys.contains("next"));
    assert_eq!(reach.keyless_read, Some(KeylessRead::Enumerable));
}

#[test]
fn a_structure_cell_keeps_raw_captures_without_running_its_body() {
    let index = RealmIndex::new(&[
        row("Owner", RealmKind::Namespace, &["Holder"], HOLD, "{}"),
        row(
            "Holder",
            RealmKind::Method,
            &["Captured", "Uncalled"],
            &[],
            r#"{"captures":[{"kind":"raw_reference","value":"Captured"}]}"#,
        ),
        row("Captured", RealmKind::Exotic, &[], &[], "{}"),
        row("Uncalled", RealmKind::Constructor, &[], &[], "{}"),
    ]);
    let mut reach = RealmReach::default();
    reach.own("Owner");
    reach.close(&index);
    assert!(reach.structure.contains("Holder"));
    assert!(reach.names.contains("Captured"));
    assert!(!reach.names.contains("Uncalled"));
    let once = reach.clone();
    reach.close(&index);
    assert_eq!(reach, once);
}

#[test]
fn keyed_calls_record_the_visible_key_and_escaped_callables_record_any_key() {
    const DEFINE: &[RealmOwn] = &[RealmOwn {
        key: "defineProperty",
        symbol: false,
        value: RealmOwnValue::Reference("Object.defineProperty"),
    }];
    let index = RealmIndex::new(&[
        row(
            "Object",
            RealmKind::Constructor,
            &["Object.defineProperty"],
            DEFINE,
            "{}",
        ),
        row(
            "Object.defineProperty",
            RealmKind::Method,
            &[],
            &[],
            r#"{"flags":["keyed_arguments"]}"#,
        ),
    ]);
    let parse = |source| perry_parser::parse_typescript(source, "keys.ts").unwrap();
    let direct = scan(
        &parse("function f(o) { Object.defineProperty(o, 'next', {}); }"),
        &index,
    );
    assert_eq!(direct.own_keys, BTreeSet::from([Some("next".into())]));
    assert!(!direct.any_receiver_key);
    let escaped = scan(&parse("const define = Object.defineProperty;"), &index);
    assert!(escaped.own_keys.contains(&None));
    assert!(escaped.any_receiver_key);
    let unknown = scan(
        &parse("function f(o,k) { Object.defineProperty(o, k, {}); }"),
        &index,
    );
    assert!(unknown.own_keys.contains(&None));
    assert!(!unknown.any_receiver_key);
}

#[test]
fn inherited_reads_own_the_member_and_accessors_do_not_prove_the_returned_holder() {
    const GET: &[RealmOwn] = &[RealmOwn {
        key: "value",
        symbol: false,
        value: RealmOwnValue::Accessor {
            get: Some("Getter"),
            set: None,
        },
    }];
    const GETTER_MEMBERS: &[RealmOwn] = &[RealmOwn {
        key: "next",
        symbol: false,
        value: RealmOwnValue::Reference("Getter.next"),
    }];
    let mut child = row("Child", RealmKind::Namespace, &["Holder"], GET, "{}");
    child.proto = Some("Holder");
    let index = RealmIndex::new(&[
        child,
        row("Holder", RealmKind::Prototype, &["Holder.next"], NEXT, "{}"),
        row("Holder.next", RealmKind::Method, &[], &[], "{}"),
        row("Getter", RealmKind::Method, &[], GETTER_MEMBERS, "{}"),
        row("Getter.next", RealmKind::Method, &[], &[], "{}"),
    ]);
    let module =
        perry_parser::parse_typescript("Child.next; Child.value.next;", "chain.ts").unwrap();
    let reach = scan(&module, &index);
    assert!(reach.own_names.contains("Holder.next"));
    assert!(reach.own_names.contains("Getter"));
    assert!(!reach.own_names.contains("Getter.next"));
    assert!(reach.receiver_keys.contains("next"));
}

#[test]
fn named_native_imports_own_the_imported_member() {
    const EXPORTED: &[RealmOwn] = &[RealmOwn {
        key: "Worker",
        symbol: false,
        value: RealmOwnValue::Reference("Worker"),
    }];
    let mut module = row(
        "node:workers",
        RealmKind::Namespace,
        &["Worker"],
        EXPORTED,
        "{}",
    );
    module.module = Some("workers");
    let index = RealmIndex::new(&[
        module,
        row("Worker", RealmKind::Constructor, &["Body"], &[], "{}"),
        row("Body", RealmKind::Exotic, &[], &[], "{}"),
    ]);
    let module = perry_parser::parse_typescript(
        "import { Worker as W } from 'node:workers'; new W();",
        "imports.ts",
    )
    .unwrap();
    let reach = scan(&module, &index);
    assert!(reach.own_names.contains("Worker"));
    assert!(reach.names.contains("Body"));
}

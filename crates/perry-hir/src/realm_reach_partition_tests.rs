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
    structure.names.insert("Holder".into());
    structure.close(&index);
    assert!(structure.structure.contains("Holder"));
    assert_eq!(
        structure.names.contains("Holder.next"),
        structure.keeps_member(
            &index,
            index.last("Holder").unwrap(),
            "next",
            "Holder.next",
            false
        )
    );
    assert!(!structure.names.contains("Holder.next"));
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
    reach.names.insert("Holder".into());
    reach.close(&index);
    reach.assert_whole_fixpoint(&index);
    let once = reach.clone();
    reach.close(&index);
    assert_eq!(reach, once);
    reach.receiver_keys.insert("next".into());
    reach.close(&index);
    assert!(reach.whole.contains("Holder.next") && reach.names.contains("Body"));
    reach.assert_whole_fixpoint(&index);
    let once = reach.clone();
    reach.close(&index);
    assert_eq!(reach, once);
}
#[test]
fn a_member_enters_reach_if_and_only_if_its_slot_is_kept() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.own("Owner");
    reach.close(&index);
    assert!(!reach.names.contains("Holder"));
    assert!(!reach.names.contains("Holder.next"));
    reach.keyless_read = Some(KeylessRead::All);
    reach.close(&index);
    assert!(reach.structure.contains("Holder"));
    assert!(reach.structure.contains("Holder.next"));
    assert!(!reach.names.contains("Body"));
    for holder in reach.names.iter().filter_map(|n| index.last(n)) {
        for (key, symbol, value) in holder.members() {
            assert_eq!(
                reach.names.contains(value),
                reach.keeps_member(
                    &index,
                    holder,
                    key,
                    value,
                    holder.member_enumerable(key, symbol)
                )
            );
        }
    }
    reach.assert_whole_fixpoint(&index);
}
#[test]
fn constructor_prototype_is_kept_by_its_bind_read() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.own("Ctor");
    reach.close(&index);
    assert!(reach.structure.contains("Ctor.prototype"));
    assert!(reach.keeps_member(
        &index,
        index.last("Ctor").unwrap(),
        "prototype",
        "Ctor.prototype",
        false
    ));
}
#[test]
fn keyless_reads_keep_values_without_following_their_bodies() {
    let index = index();
    let mut reach = RealmReach::default();
    reach.names.insert("Holder".into());
    reach.close(&index);
    assert!(!reach.keeps_member(
        &index,
        index.last("Holder").unwrap(),
        "next",
        "Holder.next",
        true
    ));
    reach.keyless_read = Some(KeylessRead::Enumerable);
    assert!(reach.keeps_member(
        &index,
        index.last("Holder").unwrap(),
        "next",
        "Holder.next",
        true
    ));
    assert!(!reach.keeps_member(
        &index,
        index.last("Holder").unwrap(),
        "next",
        "Holder.next",
        false
    ));
    reach.keyless_read = Some(KeylessRead::All);
    assert!(reach.keeps_member(
        &index,
        index.last("Holder").unwrap(),
        "next",
        "Holder.next",
        false
    ));
    reach.close(&index);
    assert!(reach.structure.contains("Holder.next"));
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
    reach.names.insert("Holder".into());
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
        RealmEntry {
            // A declared keyed body, rather than an unknown callee: this
            // test isolates the visible-key rule from unknown-code escape.
            effects: &[crate::realm_manifest::RealmEffect::Reads(
                std::borrow::Cow::Borrowed(&[
                    crate::realm_manifest::RealmSlot {
                        root: crate::realm_manifest::RealmSlotRoot::Arg(0),
                        steps: std::borrow::Cow::Borrowed(&[]),
                    },
                    crate::realm_manifest::RealmSlot {
                        root: crate::realm_manifest::RealmSlotRoot::Arg(2),
                        steps: std::borrow::Cow::Borrowed(&[]),
                    },
                ]),
            )],
            ..row(
                "Object.defineProperty",
                RealmKind::Method,
                &[],
                &[],
                r#"{"flags":["keyed_arguments"]}"#,
            )
        },
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

#[test]
fn declared_coercions_read_field_keys_without_unknown_code_escape() {
    use crate::realm_manifest::{
        RealmEffect, RealmProtocol, RealmSlot, RealmSlotRoot, RealmSlotStep,
    };
    use std::borrow::Cow;
    const EFFECTS: &[RealmEffect] = &[RealmEffect::Coerce {
        slot: RealmSlot {
            root: RealmSlotRoot::Arg(0),
            steps: Cow::Borrowed(&[RealmSlotStep::Field(Cow::Borrowed("target"))]),
        },
        protocol: RealmProtocol::HasInstance,
    }];
    let index = RealmIndex::new(&[RealmEntry {
        effects: EFFECTS,
        ..row("Host", RealmKind::Function, &[], &[], "{}")
    }]);
    let parsed =
        perry_parser::parse_typescript("function f(o) { Host(o); }", "coerce.cjs").unwrap();
    let r = scan(&parsed, &index);
    assert_eq!(
        r.receiver_keys,
        BTreeSet::from(["target".into(), "@@hasInstance".into(), "prototype".into()])
    );
    assert!(!r.chain_read);
    assert!(!r.any_receiver_key);
}

#[test]
fn a_declared_prototype_key_keeps_the_slot_without_owning_its_value() {
    let index = index();
    let mut r = RealmReach::default();
    r.names.insert("Ctor".into());
    r.receiver_keys.insert("prototype".into());
    r.close(&index);
    assert!(r.structure.contains("Ctor.prototype"), "{r:?}");
    assert!(!r.whole.contains("Ctor.prototype"));
    let holder = index.last("Ctor").unwrap();
    assert!(r.keeps_member(&index, holder, "prototype", "Ctor.prototype", false));
    r.assert_whole_fixpoint(&index);
}

#[test]
fn unknown_receiver_key_keeps_members_without_exposing_a_prototype() {
    const METHODS: &[RealmOwn] = &[RealmOwn {
        key: "m",
        symbol: false,
        value: RealmOwnValue::Reference("String.prototype.m"),
    }];
    let index = RealmIndex::new(&[
        RealmEntry {
            proto: None,
            ..row(
                "String",
                RealmKind::Constructor,
                &["String.prototype"],
                &[RealmOwn {
                    key: "prototype",
                    symbol: false,
                    value: RealmOwnValue::Reference("String.prototype"),
                }],
                r#"{"constructor":{}}"#,
            )
        },
        RealmEntry {
            proto: Some("Object.prototype"),
            ..row(
                "String.prototype",
                RealmKind::Prototype,
                &["String.prototype.m"],
                METHODS,
                "{}",
            )
        },
        row(
            "Object.prototype",
            RealmKind::Prototype,
            &["Object.prototype.__proto__.<get>"],
            &[RealmOwn {
                key: "__proto__",
                symbol: false,
                value: RealmOwnValue::Reference("Object.prototype.__proto__.<get>"),
            }],
            "{}",
        ),
        row("String.prototype.m", RealmKind::Method, &[], &[], "{}"),
        row(
            "Object.prototype.__proto__.<get>",
            RealmKind::Method,
            &[],
            &[],
            "{}",
        ),
    ]);
    let ast =
        perry_parser::parse_typescript("function f(o,k) { return o[k]; }", "unknown.ts").unwrap();
    let mut r = scan(&ast, &index);
    r.chain_roots.insert("String.prototype".into());
    r.close(&index);
    assert!(r.any_receiver_key);
    assert!(r.structure.contains("String.prototype"), "{r:?}");
    assert!(!r.whole.contains("String.prototype"));
    assert!(r.whole.contains("String.prototype.m"));
    assert!(!r.names.contains("Object.prototype.__proto__.<get>"));
    r.assert_whole_fixpoint(&index);
    let once = r.clone();
    r.close(&index);
    assert_eq!(r, once);
    r.receiver_keys.insert("__proto__".into());
    r.close(&index);
    assert!(r.whole.contains("String.prototype"));
    assert!(r.whole.contains("Object.prototype.__proto__.<get>"));
}

#[test]
fn unknown_key_on_named_holder_keeps_every_declared_member_f5() {
    const MEMBERS: &[RealmOwn] = &[
        RealmOwn {
            key: "from",
            symbol: false,
            value: RealmOwnValue::Reference("Buffer.from"),
        },
        RealmOwn {
            key: "alloc",
            symbol: false,
            value: RealmOwnValue::Reference("Buffer.alloc"),
        },
    ];
    let index = RealmIndex::new(&[
        row(
            "Buffer",
            RealmKind::Namespace,
            &["Buffer.from", "Buffer.alloc"],
            MEMBERS,
            "{}",
        ),
        row("Buffer.from", RealmKind::Method, &[], &[], "{}"),
        row("Buffer.alloc", RealmKind::Method, &[], &[], "{}"),
    ]);
    let ast =
        perry_parser::parse_typescript("function f(k) { return Buffer[k]; }", "f5.ts").unwrap();
    let r = scan(&ast, &index);
    let holder = index.last("Buffer").unwrap();
    for (key, symbol, value) in holder.members() {
        assert!(r.whole.contains(value));
        assert!(r.keeps_member(
            &index,
            holder,
            key,
            value,
            holder.member_enumerable(key, symbol)
        ));
    }
}

#[test]
fn unknown_key_can_set_a_prototype_only_after_setprototypeof_is_held() {
    const SET: &[RealmOwn] = &[RealmOwn {
        key: "setPrototypeOf",
        symbol: false,
        value: RealmOwnValue::Reference("Object.setPrototypeOf"),
    }];
    let index = RealmIndex::new(&[
        row(
            "Object",
            RealmKind::Constructor,
            &["Object.setPrototypeOf"],
            SET,
            "{}",
        ),
        row("Object.setPrototypeOf", RealmKind::Method, &[], &[], "{}"),
    ]);
    let mut r = RealmReach::default();
    r.any_receiver_key = true;
    r.chain_keys.insert(Some("next".into()));
    r.close(&index);
    assert!(!r.chain_keys.is_empty());
    assert!(!r.receiver_keys.contains("next"));
    r.names.insert("Object".into());
    r.close(&index);
    assert!(r.names.contains("Object.setPrototypeOf"));
    assert!(r.chain_keys.is_empty());
    assert!(r.receiver_keys.contains("next"));
    let once = r.clone();
    r.close(&index);
    assert_eq!(r, once);
}

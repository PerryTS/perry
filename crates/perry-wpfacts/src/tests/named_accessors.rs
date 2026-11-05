//! ACK: `Object.defineProperties(target, map)` installs one accessor per key
//! of the map's literal templates (a literal with exactly its birth keys);
//! only a map with a computed key, a key added later or an unknown value
//! installs under a computed key ("any key"). `Object.defineProperty` with a
//! constant key is named the same way.

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

/// Every live read of `key` is proven (a getter no read runs has none).
fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let dead =
        |s: &&crate::SiteFacts| matches!(&s.verdict, Verdict::No(w) if w.starts_with("dead site"));
    let ss: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key)
        .filter(|s| !dead(s))
        .collect();
    !ss.is_empty()
        && ss
            .iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// Per unit no read is lost to an accessor under a computed key. (The
/// installer's unit reads another unit's prototype, which publishes it
/// beyond its contract: the unit boundary, not the install, decides the
/// per-unit verdict.)
fn no_any_key_accessor(pf: &ProgramFacts) -> bool {
    let any = |s: &crate::SiteFacts| matches!(&s.verdict, Verdict::No(w) if w.contains("accessor under a computed key"));
    !pf.sites.iter().any(any)
        && !pf.templates.iter().any(|t| {
            [&t.escape, &t.chain].iter().any(|r| {
                r.as_deref()
                    .is_some_and(|r| r.contains("accessor under a computed key"))
            })
        })
}

/// Per unit never claims a site whole program does not.
fn never_stronger(src: &str) {
    let (u, w) = (units(src), facts(src));
    for s in &u.sites {
        if matches!(s.verdict, Verdict::No(_)) {
            continue;
        }
        let ws = w
            .sites
            .iter()
            .find(|x| (x.module, x.byte_offset, &x.key) == (s.module, s.byte_offset, &s.key))
            .unwrap_or_else(|| panic!("{s:?}\n{}", w.dump_text()));
        assert!(
            !matches!(ws.verdict, Verdict::No(_)),
            "{s:?}\n{}",
            w.dump_text()
        );
    }
}

/// tsc's `enableDebugInfo`: debug getters installed on a constructor's
/// prototype from a literal map, reachable but never read by the hot code.
const CLASS_MAP: &str = r#"
    class Node { kind: number; constructor(k: number) { this.kind = k; } }
    function enableDebugInfo() {
        Object.defineProperties(Node.prototype, {
            __dbgKind: { get() { return "k" + (this as any).kind; } },
            __dbgFlags: { get() { return 0; }, enumerable: false },
        });
    }
    enableDebugInfo();
    function rd(n: Node) { return n.kind; }
    console.log(rd(new Node(1)) + rd(new Node(2)));
"#;

const BOUND_MAP: &str = r#"
    class Node { kind: number; constructor(k: number) { this.kind = k; } }
    const debugMembers = {
        __dbgKind: { get() { return "k" + (this as any).kind; } },
    };
    function enableDebugInfo() { Object.defineProperties(Node.prototype, debugMembers); }
    enableDebugInfo();
    function rd(n: Node) { return n.kind; }
    console.log(rd(new Node(1)) + rd(new Node(2)));
"#;

#[test]
fn a_literal_map_installs_named_accessors() {
    let w = facts(CLASS_MAP);
    assert!(proven(&w, "kind"), "{}", w.dump_text());
    assert!(no_any_key_accessor(&w), "{}", w.dump_text());
    let u = units(CLASS_MAP);
    assert!(no_any_key_accessor(&u), "{}", u.dump_text());
}

#[test]
fn a_map_in_a_const_binding_installs_named_accessors() {
    let w = facts(BOUND_MAP);
    assert!(proven(&w, "kind"), "{}", w.dump_text());
    let u = units(BOUND_MAP);
    assert!(no_any_key_accessor(&u), "{}", u.dump_text());
}

#[test]
fn a_computed_key_in_the_map_answers_any_key() {
    for src in [
        // A computed key in the literal.
        r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        const k = String(Math.random());
        Object.defineProperties(Node.prototype, { [k]: { get() { return 1; } } });
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
        "#,
        // A literal stored into under a computed key before the install.
        r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        const m: any = { __dbg: { get() { return 1; } } };
        m[String(Math.random())] = { get() { return 2; } };
        Object.defineProperties(Node.prototype, m);
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
        "#,
        // A map that gains a key after birth.
        r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        const m: any = { __dbg: { get() { return 1; } } };
        m.kind = { get() { return 2; } };
        Object.defineProperties(Node.prototype, m);
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
        "#,
        // A map from unmodeled code.
        r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        Object.defineProperties(Node.prototype, JSON.parse("{}"));
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
        "#,
    ] {
        let w = facts(src);
        assert!(!proven(&w, "kind"), "{src}\n{}", w.dump_text());
    }
}

#[test]
fn a_named_accessor_is_still_invoked_on_its_key() {
    let src = r#"
        class Value { x = 1; }
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        Object.defineProperties(Node.prototype, {
            __dbg: { get() { return new Value(); } },
        });
        const n: any = new Node(1);
        console.log(n.__dbg.x + n.kind);
    "#;
    let w = facts(src);
    let x: Vec<_> = w.sites.iter().filter(|s| s.key == "x").collect();
    assert!(
        !x.is_empty()
            && x.iter()
                .all(|s| s.receivers.iter().any(|r| r.ends_with(":Value"))),
        "{}",
        w.dump_text()
    );
    // The getter's key is an accessor read, never a proven data load.
    assert!(!proven(&w, "__dbg"), "{}", w.dump_text());
    assert!(proven(&w, "kind"), "{}", w.dump_text());
}

#[test]
fn a_data_descriptor_in_the_map_stores_under_its_key() {
    let src = r#"
        class Value { x = 1; }
        const target: any = {};
        Object.defineProperties(target, { v: { value: new Value(), writable: true } });
        console.log(target.v.x);
    "#;
    let w = facts(src);
    let x: Vec<_> = w.sites.iter().filter(|s| s.key == "x").collect();
    assert!(
        !x.is_empty()
            && x.iter()
                .all(|s| s.receivers.iter().any(|r| r.ends_with(":Value"))),
        "{}",
        w.dump_text()
    );
}

#[test]
fn a_constant_key_binding_is_a_named_install() {
    let src = r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        const K = "__dbg";
        Object.defineProperty(Node.prototype, K, { get() { return 1; } });
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
    "#;
    let w = facts(src);
    assert!(proven(&w, "kind"), "{}", w.dump_text());
}

#[test]
fn per_unit_is_never_stronger_than_whole_program() {
    never_stronger(CLASS_MAP);
    never_stronger(BOUND_MAP);
}

#[test]
fn a_class_accessor_under_a_constant_key_is_named() {
    let src = r#"
        const K = "__dbg";
        class Node {
            kind: number;
            constructor(k: number) { this.kind = k; }
            get [K]() { return this.kind + 1; }
        }
        function rd(n: Node) { return n.kind; }
        const n: any = new Node(2);
        console.log(rd(new Node(1)) + n.__dbg);
    "#;
    let w = facts(src);
    assert!(proven(&w, "kind"), "{}", w.dump_text());
    assert!(no_any_key_accessor(&w), "{}", w.dump_text());
    never_stronger(src);
}

#[test]
fn a_class_accessor_under_a_computed_key_answers_any_key() {
    let src = r#"
        const K = String(Math.random());
        class Node {
            kind: number;
            constructor(k: number) { this.kind = k; }
            get [K]() { return 1; }
        }
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
    "#;
    let w = facts(src);
    assert!(!proven(&w, "kind"), "{}", w.dump_text());
}

#[test]
fn a_prototype_accessor_shadows_the_chain_for_calls() {
    // The accessor on Node's prototype (a key of the chain that is no data
    // slot) shadows the function Base's prototype holds: the call runs what
    // the getter returns, never that function alone.
    let src = r#"
        class Base {}
        (Base.prototype as any).dbg = function baseDbg() { return 1; };
        class Node extends Base { kind = 1; }
        Object.defineProperty(Node.prototype, "dbg", { get() { return () => 2; } });
        const n = new Node();
        console.log(n.dbg() + n.kind);
    "#;
    let w = facts(src);
    let calls: Vec<_> = w
        .sites
        .iter()
        .filter(|s| s.key == "dbg" && s.is_call)
        .collect();
    assert!(
        !calls.is_empty()
            && calls
                .iter()
                .all(|s| !(matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)
                    && s.targets.iter().all(|t| t.contains("baseDbg")))),
        "{}",
        w.dump_text()
    );
    assert!(proven(&w, "kind"), "{}", w.dump_text());
}

#[test]
fn a_value_defined_on_a_prototype_is_no_instance_slot() {
    // A non-writable `kind` on the prototype: the constructor's store does
    // not create an own `kind` (it throws, or fails silently in sloppy
    // code), so `kind` is no definite birth slot of Node.
    let src = r#"
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        Object.defineProperty(Node.prototype, "kind", { value: 0 });
        function rd(n: Node) { return n.kind; }
        console.log(rd(new Node(1)));
    "#;
    let w = facts(src);
    assert!(!proven(&w, "kind"), "{}", w.dump_text());
    let node = w
        .templates
        .iter()
        .find(|t| t.display == "t.ts:Node")
        .expect("Node");
    assert!(
        node.fields
            .iter()
            .all(|f| f.key != "kind" || f.slot.is_none()),
        "{}",
        w.dump_text()
    );
}

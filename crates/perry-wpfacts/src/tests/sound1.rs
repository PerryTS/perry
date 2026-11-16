//! TRACE6017 (skin m49k, head 29f2d4b1b8): the 7,518-site root "Vector3"
//! (tok 6017) enters through GLTFLoader.js:4317 `node.pivot = new Vector3()`
//! with `node` = a value of `this.nodeCache[nodeIndex]` (GLTFLoader.js:4349).
//! The index is a JSON value (external GLB JSON), and a JSON array key is
//! ToPropertyKey -> Array.prototype.toString -> join (ECMA-262 7.1.19,
//! 23.1.3.36, 23.1.3.18): any string, so `cache[k]` may be an
//! Object.prototype method (20.1.3) and the store puts a program object on
//! that builtin. The builtin is held by unknown code only if unknown code
//! holds an ordinary object (its chain reaches Object.prototype, 10.1.8.1).
//! `V::probe` reads `this.zz`: proven iff V.prototype never escapes.
use super::*;

fn v_escapes(host: &str) -> bool {
    let src = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        const cache: any = {{}};
        const k = json.nodes[0];
        const node: any = cache[k] !== undefined ? cache[k] : {{ userData: {{}} }};
        if (node.userData.pivot !== undefined) {{ node.pivot = new V(); }}
        {host}
        console.log(new V().probe());
        "#
    );
    let v = site(&facts(&src), "zz").verdict.clone();
    matches!(v, Verdict::No(_))
}

/// No unknown code holds anything: the builtin that `node` may denote is
/// held by no unknown code, so V never escapes. RED on 29f2d4b1b8 means a
/// hop of the chain is wrong without any escaped holder.
#[test]
fn no_escaped_holder_keeps_v_proven() {
    assert!(!v_escapes(""));
}

/// Control (real host value): unknown code gets an ordinary object, so it
/// holds Object.prototype methods through its chain; one of them may carry
/// `pivot` = a V, so V escapes.
#[test]
fn control_escaped_object_reveals_proto_methods() {
    assert!(v_escapes("(globalThis as any).hostTake({ a: 1 });"));
}

/// Control (program value really reachable from unknown code): the V is
/// stored on an object the host holds.
#[test]
fn control_store_on_host_held_object() {
    assert!(v_escapes("const h: any = {}; h.pivot = new V(); (globalThis as any).hostTake(h);"));
}

/// SOUND1 probe (diagnostic, not a gate): which hosted value makes V escape.
#[test]
fn sound1_probe_variants() {
    for host in [
        "",
        "(globalThis as any).hostTake({ a: 1 });",
        "(globalThis as any).hostTake(Object.prototype);",
        "(globalThis as any).hostTake(Object);",
        "(globalThis as any).hostTake(Object.prototype.toString);",
        "(globalThis as any).hostTake(Object.getPrototypeOf({ a: 1 }).toString);",
        "(globalThis as any).hostTake(() => 1);",
        "(globalThis as any).hostTake(globalThis);",
    ] {
        eprintln!("SOUND1 v_escapes={} host={host}", v_escapes(host));
    }
}

/// SOUND1 probe: escape-first records of the failing control.
#[test]
fn sound1_probe_records() {
    let src = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const cache: any = {};
        const k = json.nodes[0];
        const node: any = cache[k] !== undefined ? cache[k] : { userData: {} };
        if (node.userData.pivot !== undefined) { node.pivot = new V(); }
        (globalThis as any).hostTake({ a: 1 });
        console.log(new V().probe());
        "#;
    let pf = facts_with(src, FactsConfig { diagnostics: true, ..Default::default() });
    for l in pf.diagnostics.iter().flat_map(|d| d.1.iter()) {
        if l.contains("escape-first") || [2,3,27,28,37,47,48].iter().any(|i| l.contains(&format!("\"record\":\"token\",\"id\":{i},"))) {
            eprintln!("SOUND1REC {}", &l[..l.len().min(400)]);
        }
    }
}

/// SOUND1 RED (real hole, verdict 3). The model: unknown code holds what it
/// is handed, every field of what it holds and the prototypes on its chain
/// (solve/escape.rs `escape_body`: `all_fields` / `any_key_node` / `proto_node`
/// -> TopSink), and may store any key into a function it holds (DECISIONS
/// 241; escaped builtin functions: `escaped-native-fields`). It never mutates
/// an intrinsic prototype itself (escape.rs "Runtime-owned objects do not
/// escape"), but it READS its members (10.1.8.1 OrdinaryGet): a member is a
/// function object it then holds, whose own keys it may write (10.2, 10.1.9).
/// Run-time witness: argv[2] = "toString"; host does
///   `const f = Object.getPrototypeOf(o).toString; f.userData = { pivot: 1 };`
/// then later `f.pivot.zz = "s"; f.pivot.probe();`: `this.zz` is a string.
fn v_escapes_argv(host: &str) -> bool {
    let src = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        {host}
        const cache: any = {{}};
        const node: any = cache[process.argv[2] ?? "x"];
        if (node !== undefined && node.userData !== undefined && node.userData.pivot !== undefined) {{ node.pivot = new V(); }}
        console.log(new V().probe());
        "#
    );
    matches!(site(&facts(&src), "zz").verdict, Verdict::No(_))
}

/// RED: an ordinary object handed to unknown code; its chain is Object.prototype.
#[test]
fn red_sound1_plain_object_chain_members_held() {
    assert!(v_escapes_argv("(globalThis as any).hostTake({ a: 1 });"));
}

/// RED: Object.prototype itself handed to unknown code.
#[test]
fn red_sound1_object_prototype_members_held() {
    assert!(v_escapes_argv("(globalThis as any).hostTake(Object.prototype);"));
}

/// RED: a program function handed to unknown code; its chain is
/// Function.prototype -> Object.prototype.
#[test]
fn red_sound1_function_chain_members_held() {
    assert!(v_escapes_argv("(globalThis as any).hostTake(() => 1);"));
}

/// Control (green): the member itself handed: the hop cache[k] -> member ->
/// guarded store is modeled; only the reach from its holder is missing.
#[test]
fn control_sound1_member_handed() {
    assert!(v_escapes_argv("(globalThis as any).hostTake(Object.prototype.toString);"));
}

/// Control (green): no unknown code holds anything: V stays proven.
#[test]
fn control_sound1_no_unknown_code() {
    assert!(!v_escapes_argv(""));
}

/// HELDMEM: `v_escapes_argv` with the member read off `read` (an expression
/// of the intrinsic whose member the program may store a V on).
fn v_escapes_from(host: &str, read: &str) -> bool {
    let src = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        {host}
        const node: any = ({read})[process.argv[2] ?? "x"];
        if (node !== undefined && node.userData !== undefined && node.userData.pivot !== undefined) {{ node.pivot = new V(); }}
        console.log(new V().probe());
        "#
    );
    matches!(site(&facts(&src), "zz").verdict, Verdict::No(_))
}

/// HELDMEM 3(a): an instance of a class extending Array. Unknown code holds
/// its chain A.prototype -> Array.prototype, whose `constructor` is Array,
/// and `Object.getPrototypeOf(A) === Array` (ECMA-262 15.7.14 step 8.b):
/// argv[2] = "from"; host: `a.constructor.__proto__.from.userData = {pivot:1}`,
/// later `Array.from.pivot.zz = "s"`.
#[test]
fn red_heldmem_extends_array_ctor_members_held() {
    assert!(v_escapes_from(
        "class A extends Array {} (globalThis as any).hostTake(new A());",
        "Array as any"
    ));
}

/// HELDMEM 3(a): the class itself: its [[Prototype]] is Array (reached along
/// the chain only, escape.rs `chain_escape && builtin_function`).
#[test]
fn red_heldmem_extends_array_class_ctor_members_held() {
    assert!(v_escapes_from(
        "class A extends Array {} (globalThis as any).hostTake(A);",
        "Array as any"
    ));
}

/// HELDMEM 3(a): Array.prototype's members through the instance's chain:
/// argv[2] = "push"; host: `Object.getPrototypeOf(Object.getPrototypeOf(a)).push.userData = {pivot:1}`.
#[test]
fn red_heldmem_extends_array_proto_members_held() {
    assert!(v_escapes_from(
        "class A extends Array {} (globalThis as any).hostTake(new A());",
        "Array.prototype as any"
    ));
}

/// Control 3(a): the same class, never given to unknown code.
#[test]
fn control_heldmem_extends_array_not_given() {
    assert!(!v_escapes_from(
        "class A extends Array {} console.log(new A().length);",
        "Array as any"
    ));
}

/// HELDMEM 3(b): the global object handed: unknown code reads
/// `globalThis.Object.prototype.toString`.
#[test]
fn red_heldmem_global_object_members_held() {
    assert!(v_escapes_argv("(globalThis as any).hostTake(globalThis);"));
}

/// HELDMEM 3(c): a Map instance: `Object.getPrototypeOf(m).get.userData = ...`.
#[test]
fn red_heldmem_map_proto_members_held() {
    assert!(v_escapes_from("(globalThis as any).hostTake(new Map());", "Map.prototype as any"));
}

/// HELDMEM 3(c): a Set instance.
#[test]
fn red_heldmem_set_proto_members_held() {
    assert!(v_escapes_from("(globalThis as any).hostTake(new Set());", "Set.prototype as any"));
}

/// HELDMEM 3(c): a Promise instance.
#[test]
fn red_heldmem_promise_proto_members_held() {
    assert!(v_escapes_from(
        "(globalThis as any).hostTake(Promise.resolve(1));",
        "Promise.prototype as any"
    ));
}

/// Control 3(c): Map.prototype members with no unknown code.
#[test]
fn control_heldmem_map_no_unknown_code() {
    assert!(!v_escapes_from("console.log(new Map().size);", "Map.prototype as any"));
}

/// Precision (4): unknown code is called with a primitive only. Recorded,
/// see NOTE ## HELDMEM: the model's "unknown code already holds them"
/// (runtime-owned objects) says unknown code holds every intrinsic.
#[test]
fn probe_heldmem_primitive_only() {
    eprintln!("SOUND1 primitive_only v_escapes={}", v_escapes_argv("(globalThis as any).hostTake(1);"));
}

/// Precision (4): an object escapes, another program object W never reaches
/// unknown code: W's field read stays proven (the held intrinsic members
/// give unknown code no path to W).
#[test]
fn control_heldmem_unreached_object_stays_proven() {
    let src = r#"
        class V { zz = 1; probe() { return this.zz; } }
        class W { ww = 1; read() { return this.ww; } }
        (globalThis as any).hostTake({ a: 1 });
        const cache: any = {};
        const node: any = cache[process.argv[2] ?? "x"];
        if (node !== undefined && node.userData !== undefined && node.userData.pivot !== undefined) { node.pivot = new V(); }
        const w = new W();
        console.log(new V().probe(), w.read());
        "#;
    assert!(!matches!(site(&facts(src), "ww").verdict, Verdict::No(_)));
}

/// HELDMEM2 RED: unknown code handed only a primitive (and its receiver,
/// the global object, which by itself made nothing held) holds the realm: it
/// can write `Object.prototype.toString.userData = { pivot: 1 }` itself
/// (escape.rs "unknown code already holds them"), so V escapes.
#[test]
fn red_realm_held_primitive_only() {
    assert!(v_escapes_argv("(globalThis as any).hostTake(1);"));
}

/// HELDMEM2: an unknown-key read of a plain object (an `Object.prototype`
/// method or undefined), then a read of that method`s `userData`: ⊤ iff
/// unknown code holds the realm. `true` = the `ww` read is not proven.
fn builtin_member_field_top(host: &str) -> bool {
    let src = format!(
        r#"
        class W {{ ww = 1; }}
        {host}
        const o: any = {{}};
        const f: any = o[process.argv[2] ?? "x"];
        const u: any = (f !== undefined && f.userData !== undefined) ? f.userData : new W();
        console.log(u.ww);
        "#
    );
    matches!(site(&facts(&src), "ww").verdict, Verdict::No(_))
}

/// HELDMEM2 RED: the program reads what unknown code may have stored on an
/// intrinsic function it holds: ⊤.
#[test]
fn red_realm_held_primitive_only_member_fields_top() {
    assert!(builtin_member_field_top("(globalThis as any).hostTake(1);"));
}

/// HELDMEM2 control: no unknown code anywhere: the realm is held by no one,
/// V stays proven and a builtin function`s fields hold no ⊤.
#[test]
fn control_no_unknown_code_realm_not_held() {
    assert!(!v_escapes_argv(""));
    assert!(!builtin_member_field_top(""));
}

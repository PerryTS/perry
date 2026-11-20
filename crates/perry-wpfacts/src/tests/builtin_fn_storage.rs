//! Program properties of a builtin function retain their stored values.
use super::*;

const SETUP: &str = r#"
class Node { kind: number; constructor() { this.kind = 0; } }
class Diagnostic { code: number; constructor() { this.code = 7; } }
const nodes: any = [new Node()];
const node = nodes[process.argv[2] ?? "0"];
const diagnostics = [new Diagnostic()];
"#;

fn check(tail: &str, marker: &str, key: &str, want_proven: bool) {
    let src = format!("{SETUP}{tail}");
    let m = src.find(marker).expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let pf = facts(&src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!sites.is_empty(), "missing site {marker}/{key}");
    for s in sites {
        assert_eq!(
            !matches!(s.verdict, Verdict::No(_)),
            want_proven,
            "{marker}/{key}: {:?} {:?}\n{}",
            s.verdict,
            s.receivers,
            pf.dump_text()
        );
    }
}

#[test]
fn mixed_builtin_receiver_retains_stored_array() {
    check(
        r#"
node.additionalSyntacticDiagnostics = diagnostics;
console.log(diagnostics[0].code); // DIRECT
console.log(node.additionalSyntacticDiagnostics[0].code); // THROUGH
"#,
        "// DIRECT",
        "code",
        true,
    );
    check(
        r#"
node.additionalSyntacticDiagnostics = diagnostics;
console.log(node.additionalSyntacticDiagnostics[0].code); // THROUGH
"#,
        "// THROUGH",
        "code",
        true,
    );
}

#[test]
fn unknown_callee_reads_builtin_properties() {
    check(
        r#"
declare function external(x: any): void;
node.additionalSyntacticDiagnostics = diagnostics;
external(node);
console.log(diagnostics[0].code); // UNKNOWN
"#,
        "// UNKNOWN",
        "code",
        false,
    );
}

#[test]
fn builtin_copy_into_escaping_object_exposes_values() {
    check(
        r#"
declare function external(x: any): void;
node.additionalSyntacticDiagnostics = diagnostics;
const copy = Object.assign({}, node);
external(copy);
console.log(diagnostics[0].code); // COPY
"#,
        "// COPY",
        "code",
        false,
    );
}

#[test]
fn unknown_receiver_store_still_escapes() {
    check(
        r#"
declare function unknown(): any;
unknown().additionalSyntacticDiagnostics = diagnostics;
console.log(diagnostics[0].code); // TOP
"#,
        "// TOP",
        "code",
        false,
    );
}

#[test]
fn unknown_computed_read_stays_unproven() {
    check(
        r#"
node.additionalSyntacticDiagnostics = diagnostics;
console.log(node[process.argv[3] ?? "other"][0].code); // COMPUTED
"#,
        "// COMPUTED",
        "code",
        false,
    );
}

#[test]
fn unknown_overwrite_stays_unproven() {
    check(
        r#"
declare function unknown(): any;
node.additionalSyntacticDiagnostics = diagnostics;
node.additionalSyntacticDiagnostics = unknown();
console.log(node.additionalSyntacticDiagnostics[0].code); // OVERWRITE
"#,
        "// OVERWRITE",
        "code",
        false,
    );
}

#[test]
fn distinct_identified_natives_do_not_share_writes_via_any() {
    check(
        r#"
declare function unknown(): any;
const x: any = [1].map;
const y: any = [1].pop;
y.cache = new Diagnostic();
x.cache = unknown();
console.log(y.cache.code); // DISTINCT
"#,
        "// DISTINCT",
        "code",
        true,
    );
}

#[test]
fn aliased_native_members_share_written_values() {
    check(
        r#"
declare function unknown(): any;
const s = new Set();
const x: any = s.keys;
const y: any = s.values;
y.cache = new Diagnostic();
x.cache = unknown();
console.log(y.cache.code); // ALIAS
"#,
        "// ALIAS",
        "code",
        false,
    );
}

fn protocol(tail: &str) {
    check(
        &format!(
            r#"
declare function external(x: any): void;
const f: any = [1].map;
function effect(...args: any[]) {{ external(diagnostics); return "effect"; }}
{tail}
console.log(diagnostics[0].code); // EFFECT
"#
        ),
        "// EFFECT",
        "code",
        false,
    );
}

#[test]
fn stored_to_string_runs_during_conversion() {
    protocol("f.toString = effect; console.log(`${f}`);");
}

#[test]
fn stored_has_instance_runs_during_instanceof() {
    protocol("f[Symbol.hasInstance] = effect; console.log({} instanceof f);");
}

#[test]
fn stored_then_runs_during_await() {
    protocol("f.then = effect; async function run() { await f; } run();");
}

#[test]
fn stored_call_runs_as_own_method() {
    protocol("f.call = effect; f.call(null);");
}

#[test]
fn computed_store_reaches_implicit_protocol() {
    protocol("f[process.argv[3] ?? 'toString'] = effect; console.log(`${f}`);");
}

#[test]
fn stored_protocol_functions_stay_dormant_without_a_use() {
    check(
        r#"
declare function external(x: any): void;
const f: any = [1].map;
function effect(...args: any[]) { external(diagnostics); return "effect"; }
f.toString = effect;
f[Symbol.hasInstance] = effect;
f.then = effect;
f.call = effect;
console.log(diagnostics[0].code); // DORMANT
"#,
        "// DORMANT",
        "code",
        true,
    );
}

#[test]
fn builtin_constructor_retains_ordinary_property() {
    check(
        r#"
(Object as any).diagnostics = diagnostics;
console.log((Object as any).diagnostics[0].code); // CONSTRUCTOR
"#,
        "// CONSTRUCTOR",
        "code",
        true,
    );
}

#[test]
fn native_getter_returns_retained_array() {
    check(
        r#"
const f: any = Math.max;
Object.defineProperty(f, "diagnostics", { get() { return diagnostics; } });
console.log(f.diagnostics[0].code); // GETTER
"#,
        "// GETTER",
        "code",
        true,
    );
}

#[test]
fn getter_result_copied_into_native_retains_array() {
    check(
        r#"
const f: any = Math.max;
Object.assign(f, { get diagnostics() { return diagnostics; } });
console.log(f.diagnostics[0].code); // ASSIGNGET
"#,
        "// ASSIGNGET",
        "code",
        true,
    );
}

#[test]
fn native_setter_is_found_through_an_overlapping_token() {
    check(
        r#"
declare function external(x: any): void;
const f: any = [1].map;
Object.defineProperty(f, "diagnostics", { set(v: any) { external(diagnostics); } });
node.diagnostics = 1;
console.log(diagnostics[0].code); // SETTERALIAS
"#,
        "// SETTERALIAS",
        "code",
        false,
    );
}

#[test]
fn copying_native_alias_fields_to_unknown_target_exposes_values() {
    check(
        r#"
declare function unknown(): any;
const f: any = [1].map;
node.additionalSyntacticDiagnostics = diagnostics;
Object.assign(unknown(), f);
console.log(diagnostics[0].code); // COPYTOP
"#,
        "// COPYTOP",
        "code",
        false,
    );
}

#[test]
fn a_computed_protocol_store_stays_dormant_without_a_use() {
    check(
        r#"
declare function external(x: any): void;
const f: any = [1].map;
function effect(...args: any[]) { external(diagnostics); return "effect"; }
f[process.argv[3] ?? "toString"] = effect;
console.log(diagnostics[0].code); // COMPUTEDDORMANT
"#,
        "// COMPUTEDDORMANT",
        "code",
        true,
    );
}

fn field_effect(holder: &str, store: &str, use_value: &str, iterator: bool, want_proven: bool) {
    let result = if iterator {
        "({ next() { return { value: 0, done: true }; } })"
    } else {
        "\"effect\""
    };
    check(
        &format!(
            r#"
declare function input(): any;
const F: any = {holder};
const o: any = {{ n: new Diagnostic() }};
function g(...args: any[]) {{ o.n = input(); return {result}; }}
{store}
{use_value}
console.log(o.n.code); // FIELD-EFFECT
"#
        ),
        "// FIELD-EFFECT",
        "code",
        want_proven,
    );
}

fn enumerated_field_write(holder: &str, write: &str) {
    check(
        &format!(
            r#"
declare function input(): any;
const F: any = {holder};
const o: any = {{ n: new Diagnostic() }};
F.k = o;
{write}
console.log(o.n.code); // ENUM-FIELD
"#
        ),
        "// ENUM-FIELD",
        "code",
        false,
    );
}

macro_rules! implicit_field_controls {
    () => {
        #[test]
        fn to_json_direct() {
            field_effect(HOLDER, "F.toJSON = g;", "JSON.stringify(F);", false, false);
        }
        #[test]
        fn to_json_nested() {
            field_effect(HOLDER, "F.toJSON = g;", "JSON.stringify({ a: F });", false, false);
        }
        #[test]
        fn value_of_unary_plus() {
            field_effect(HOLDER, "F.valueOf = g;", "console.log(+F);", false, false);
        }
        #[test]
        fn value_of_relational() {
            field_effect(HOLDER, "F.valueOf = g;", "console.log(F < 1);", false, false);
        }
        #[test]
        fn symbol_to_primitive_template() {
            field_effect(HOLDER, "F[Symbol.toPrimitive] = g;", "console.log(`${F}`);", false, false);
        }
        #[test]
        fn symbol_to_primitive_add() {
            field_effect(HOLDER, "F[Symbol.toPrimitive] = g;", "console.log(F + \"\");", false, false);
        }
        #[test]
        fn to_string_string_constructor() {
            field_effect(HOLDER, "F.toString = g;", "console.log(String(F));", false, false);
        }
        #[test]
        fn to_string_property_key() {
            field_effect(HOLDER, "F.toString = g;", "console.log(({} as any)[F]);", false, false);
        }
        #[test]
        fn iterator_array_spread() {
            field_effect(HOLDER, "F[Symbol.iterator] = g;", "console.log([...F]);", true, false);
        }
        #[test]
        fn iterator_for_of() {
            field_effect(HOLDER, "F[Symbol.iterator] = g;", "for (const x of F) { console.log(x); }", true, false);
        }
        #[test]
        fn enumerate_object_values() {
            enumerated_field_write(HOLDER, "Object.values(F)[0].n = input();");
        }
        #[test]
        fn enumerate_object_entries() {
            enumerated_field_write(HOLDER, "Object.entries(F)[0][1].n = input();");
        }
        #[test]
        fn enumerate_object_spread() {
            enumerated_field_write(HOLDER, "const copy: any = { ...F }; copy.k.n = input();");
        }
        #[test]
        fn enumerate_object_assign() {
            enumerated_field_write(HOLDER, "Object.assign({}, F).k.n = input();");
        }
        #[test]
        fn enumerate_for_in_read() {
            enumerated_field_write(HOLDER, "for (const key in F) { F[key].n = input(); }");
        }
        #[test]
        fn accessor_direct_read() {
            field_effect(HOLDER, "Object.defineProperty(F, \"x\", { get: g });", "console.log(F.x);", false, false);
        }
        #[test]
        fn accessor_reflect_get() {
            field_effect(HOLDER, "Object.defineProperty(F, \"x\", { get: g });", "console.log(Reflect.get(F, \"x\"));", false, false);
        }
        #[test]
        fn stored_handlers_stay_dormant() {
            field_effect(HOLDER, "F.toJSON = g; F.valueOf = g; F[Symbol.toPrimitive] = g; F.toString = g; F[Symbol.iterator] = g;", "", false, true);
        }
    };
}

mod identified_push {
    use super::*;
    const HOLDER: &str = "Array.prototype.push";
    implicit_field_controls!();
}

mod object_has_own {
    use super::*;
    const HOLDER: &str = "Object.prototype.hasOwnProperty";
    implicit_field_controls!();
}

mod anonymous_max {
    use super::*;
    const HOLDER: &str = "Math.max";
    implicit_field_controls!();
}

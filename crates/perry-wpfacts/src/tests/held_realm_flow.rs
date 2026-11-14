//! HOSTHELD: the flow of a call of a builtin or host member the runtime does
//! not lock, once unknown code holds the realm (`Model::realm_held`, ECMA-262
//! 9.3). Unknown code may have replaced the member with any function it made
//! (no `LockHolder` covers a namespace, a host object or the global object:
//! perry-abi lock.rs), so `ns.f(o)` may call unknown code with `o`, which may
//! then replace `o.m`: a later `o.m()` is not the program body. The realm is
//! held by an event anywhere in the program (the model is flow-insensitive),
//! so the event may come after the call in source order. Each twin without
//! unknown code keeps `o.m()` the program body (T0Mono).
use super::*;

const HELD: &str = "declare function sink(x: any): void;\nsink(0);\n";
const OBJ: &str = "const o = { m() { return 1; } };\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the call sites of `m`, in source order.
fn m_calls(src: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "m" && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("HOSTHELD {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of m");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().any(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// `call` hands `o` to the member; then `o.m()`.
fn check(call: &str) {
    let body = format!("{OBJ}{call};\nconst r = o.m();\n");
    let free = m_calls(&body);
    assert!(mono(&free), "free twin lost the claim: {free:?}");
    let held = m_calls(&format!("{HELD}{body}"));
    assert!(!mono(&held), "held realm: {call} may call unknown code with o: {held:?}");
}

/// A member whose own rule already hands `o` to unknown code (the stream
/// keeps its chunk; a native module's argument): exact in both arms.
fn held_only(call: &str) {
    let body = format!("{OBJ}{call};\nconst r = o.m();\n");
    let held = m_calls(&format!("{HELD}{body}"));
    assert!(!mono(&held), "held realm: {call}: {held:?}");
}

/// A call perry-hir lowers to its own expression (`Expr::MathMax`,
/// `Expr::JsonStringify`, `Expr::ObjectKeys`, `Expr::ParseInt`, ...;
/// perry-hir lower/expr_call/module_static.rs): the compiled program runs
/// the runtime's operation and never reads the member off the namespace, so
/// a replacement unknown code made is not called. Exact in both arms: the
/// claim on `o.m()` stays.
fn fixed_op(call: &str) {
    let body = format!("{OBJ}{call};\nconst r = o.m();\n");
    let free = m_calls(&body);
    assert!(mono(&free), "free: {call}: {free:?}");
    let held = m_calls(&format!("{HELD}{body}"));
    assert!(mono(&held), "held: {call} is the compiled operation: {held:?}");
}

#[test] fn console_log_held() { check("console.log(o)") }
#[test] fn console_warn_held() { check("console.warn(o)") }
#[test] fn math_max_fixed_op() { fixed_op("Math.max(o as any, 1)") }
#[test] fn json_stringify_fixed_op() { fixed_op("JSON.stringify(o)") }
#[test] fn object_keys_fixed_op() { fixed_op("Object.keys(o)") }
#[test] fn reflect_ownkeys_fixed_op() { fixed_op("Reflect.ownKeys(o)") }
#[test] fn parse_int_fixed_op() { fixed_op("parseInt(o as any)") }
#[test] fn global_this_parse_int_held() { check("globalThis.parseInt(o as any)") }
#[test] fn symbol_for_fixed_op() { fixed_op("Symbol.for(o as any)") }
#[test] fn buffer_from_fixed_op() { fixed_op("Buffer.from(o as any)") }
#[test] fn process_emit_warning_fixed_op() { fixed_op("process.emitWarning(o as any)") }
#[test] fn stdout_write_held() { held_only("process.stdout.write(o as any)") }
#[test] fn performance_mark_held() { held_only("performance.mark(o as any)") }
#[test] fn math_max_value_call_held() { check("const f = Math.max; f(o as any, 1)") }
#[test] fn set_timeout_held() { check("setTimeout(() => {}, 0, o)") }

/// The realm is held by an event later in source order than the call: in a
/// function run twice, the second `console.log(o)` runs after `sink(0)`.
#[test]
fn console_log_held_later_in_source() {
    let body = format!(
        "declare function sink(x: any): void;\n{OBJ}function f() {{ console.log(o); const r = o.m(); sink(0); }}\nf();\nf();\n"
    );
    let held = m_calls(&body);
    assert!(!mono(&held), "held later in source: {held:?}");
}

// HOSTHELD2 A: a method of a built-in object whose prototype no
// `LockHolder` covers (`Buffer.prototype`, `%TypedArray%.prototype`,
// `ArrayBuffer.prototype`, `DataView.prototype`, `Error.prototype`,
// `URL.prototype`, `TextEncoder.prototype`): unknown code that holds the
// realm holds that prototype and may have put its own function under the
// key (ECMA-262 10.1.9 [[Set]] on an ordinary extensible object).
const BUF: &str = "const b = Buffer.from(\"x\");\n";
#[test] fn buffer_write_held() { check(&format!("{BUF}b.write(o as any)")) }
#[test] fn buffer_includes_held() { check(&format!("{BUF}b.includes(o as any)")) }
#[test] fn buffer_equals_held() { check(&format!("{BUF}b.equals(o as any)")) }
#[test] fn typed_array_set_held() { check("new Uint8Array(4).set(o as any)") }
#[test] fn typed_array_index_of_held() { check("new Uint8Array(4).indexOf(o as any)") }
#[test] fn array_buffer_slice_held() { check("new ArrayBuffer(4).slice(o as any)") }
#[test] fn data_view_get_held() { check("new DataView(new ArrayBuffer(4)).getInt8(o as any)") }
// A receiver perry knows is a URL (`new URL(..)`, a URL-typed local):
// perry-hir url_date_instance.rs lowers `.toJSON()` / `.toString()` to the
// href read (`UrlInstanceToJSON`, codegen `js_url_get_href`): no [[Get]] on
// `URL.prototype`, the arguments are not passed. Nothing unknown code puts
// there runs: exact in both arms.
#[test] fn url_to_json_static_exact() { locked("new URL(\"http://a\").toJSON(o as any)") }
// The same call on a receiver perry does not type: the method is the
// [[Get]] of `toJSON` on `URL.prototype`, which no `LockHolder` covers.
#[test] fn url_to_json_held() { check("const u: any = new URL(\"http://a\");\nu.toJSON(o)") }
#[test] fn url_search_params_held() { check("const p: any = new URLSearchParams(\"a=1\");\np.has(o)") }
#[test] fn shared_array_buffer_slice_held() { check("new SharedArrayBuffer(4).slice(o as any)") }
#[test] fn timer_refresh_held() { check("const t: any = setTimeout(() => 0, 0);\nt.refresh(o)") }
#[test] fn text_encoder_encode_held() { check("new TextEncoder().encode(o as any)") }

/// A locked prototype's image member (DEC 250: a Map's `set`) or a binding
/// no other code reaches: exact in both arms, the claim stays.
fn locked(call: &str) {
    let body = format!("{OBJ}{call};\nconst r = o.m();\n");
    let free = m_calls(&body);
    assert!(mono(&free), "free: {call}: {free:?}");
    let held = m_calls(&format!("{HELD}{body}"));
    assert!(mono(&held), "held: {call} is not replaceable: {held:?}");
}
/// `new Map()` reads the live global binding `Map` (perry-hir
/// lower/expr_new.rs: global_property_new_dynamic), which unknown code that
/// holds the realm may replace: the loss is real.
#[test] fn new_map_set_held() { held_only("new Map().set(1, o)") }
#[test] fn array_index_of_locked() { locked("[1].indexOf(o as any)") }

// HOSTHELD2 B: a program-owned property of the global object (ECMA-262
// 9.1.1.4, the object record of the global environment): unknown code that
// holds the realm holds `globalThis` and may replace it; a call of it may
// then call unknown code with `o`. A module's own `var`, `function`, `let`,
// `const` and `class` live in the module environment record (16.2.1.6.4
// InitializeEnvironment), which no other code reaches.
const GF: &str = "(globalThis as any).gf = (x: any) => 0;\n";
#[test] fn global_this_member_held() { held_only(&format!("{GF}(globalThis as any).gf(o)")) }
#[test] fn global_this_bare_held() { check(&format!("{GF}(gf as any)(o)")) }
#[test] fn global_this_read_value_held() { check(&format!("{GF}const g = (globalThis as any).gf; g(o)")) }
#[test] fn module_var_exact() { locked("var gv = (x: any) => 0;\ngv(o)") }
#[test] fn module_function_exact() { locked("function gd(x: any) { return 0; }\ngd(o)") }
#[test] fn module_let_exact() { locked("let gl = (x: any) => 0;\ngl(o)") }

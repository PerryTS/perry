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

//! RAWEXACT1 (2026-11-15): String.raw (ECMA-262 22.1.2.4) only Gets `raw`,
//! its `length` and elements and ToStrings them and the substitutions;
//! no method runs under an unknown key. Fixture from SRCDIAG1 (r9b root 1).
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("HD1 {key} {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// The twin without `esc` keeps `b.m()` M::m (the test can fail for the
/// right reason); with `esc`, `console.log(b)` (a realm member the runtime
/// does not lock, tests/held_realm_flow.rs) must still not hand `b` to
/// unknown code: no unknown code exists.
fn red(esc: &str) {
    let probe = "console.log(b);\nb.m();\n";
    let free = calls(&format!("{F}{probe}"), "m");
    assert!(mono(&free), "free twin lost the claim: {free:?}");
    let v = calls(&format!("{F}{esc}\n{probe}"), "m");
    assert!(mono(&v), "{esc}: b.m() stays M::m: {v:?}");
}

const F: &str = "function f() { return 1; }\n\
class M { m() { return 1; } }\nconst b = new M();\n";

/// RED: String.raw (22.1.2.4) Gets `raw`, `length`, the elements and
/// ToStrings them (7.1.17 -> 7.1.1 OrdinaryToPrimitive: `toString` of
/// Function.prototype, 20.2.3.5): `f` is never called.

/// SRCDIAG1 (r9b root 1, traced by the call diagnostic): a for-in copy fills
/// an object literal under any key from a pool that holds a builtin member
/// (String.raw); `this.add( arguments[ i ] )` (three.core.js:12643) on that
/// object calls the String.raw model, whose Protocol::Any touch on the
/// Object3D argument calls every method of it (c4601), touches their array
/// results (Array@call0, c4611: Array shift/pop under any key) and escapes
/// the EventDispatcher method an element holds: the realm is held.
#[test]
fn raw1_real_root1() {
    red("class EventDispatcher { addEventListener( type: any, listener: any ) { (this as any)._listeners = [ type, listener ]; } }\n\
class Object3D extends EventDispatcher {\n\
  children: any[] = [];\n\
  add( object: any ): any {\n\
    if ( arguments.length > 1 ) { for ( let i = 0; i < arguments.length; i ++ ) { this.add( arguments[ i ] ); } return this; }\n\
    this.children.push( object ); return this;\n\
  }\n\
  parts(): any[] { return [ EventDispatcher.prototype, (EventDispatcher.prototype as any)[ String(Math.random()) ] ]; }\n\
}\n\
const src: any = { add: String.raw, n: 1 };\n\
const dst: any = {};\n\
for ( const p in src ) { dst[ p ] = src[ p ]; }\n\
const o = new Object3D();\n\
o.add.call( dst, new Object3D(), new Object3D() );\n");
}

/// KEEP: String.raw really ToStrings each segment of `raw` (22.1.2.4 step
/// 6.b-c, 7.1.17): the segment's own `toString` runs and its write is seen.
#[test]
fn raw1_keep_segment_to_string_runs() {
    let src = "class M { m() { return 1; } }\nconst b = new M();\n\
               String.raw({ raw: { length: 1, 0: { toString() { (b as any).m = () => 2; return \x27\x27; } } } });\nb.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the segment toString replaces b.m: {v:?}");
}

/// KEEP: a substitution is ToStringed (22.1.2.4 step 6.e.ii): its own
/// `toString` runs.
#[test]
fn raw1_keep_substitution_to_string_runs() {
    let src = "class M { m() { return 1; } }\nconst b = new M();\n\
               String.raw({ raw: [\x27a\x27, \x27c\x27] }, { toString() { (b as any).m = () => 2; return \x27\x27; } });\nb.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the substitution toString replaces b.m: {v:?}");
}

/// KEEP: `raw` may be an accessor; its getter runs (Get, 7.3.2).
#[test]
fn raw1_keep_raw_getter_runs() {
    let src = "class M { m() { return 1; } }\nconst b = new M();\n\
               String.raw({ get raw() { (b as any).m = () => 2; return []; } });\nb.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the raw getter replaces b.m: {v:?}");
}

/// KEEP: the same with an Array `raw` (a template object's `raw` is one).
#[test]
fn raw1_keep_array_segment_to_string_runs() {
    let src = "class M { m() { return 1; } }\nconst b = new M();\n\
               String.raw({ raw: [{ toString() { (b as any).m = () => 2; return ''; } }] });\nb.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the segment toString replaces b.m: {v:?}");
}

/// PROBE: the program reads the literal key `0` by a Number and ToStrings it.
#[test]
fn raw1_probe_program_index_read() {
    let src = "class M { m() { return 1; } }\nconst b = new M();\n\
               const r: any = { length: 1, 0: { toString() { (b as any).m = () => 2; return ''; } } };\n\
               const i = Math.random() > 2 ? 1 : 0;\nString(r[i]);\nb.m();\n";
    let v = calls(src, "m");
    assert!(!mono(&v), "the program read: {v:?}");
}

//! three.js GLTFLoader `GLTFParser.getDependency` (GLTFLoader.js ~2926):
//! `const cacheKey = type + ':' + index; let d = this.cache.get(cacheKey);
//! if (!d) { d = ...; this.cache.add(cacheKey, d); }`, the registry's `get`
//! a `return objects[key]`. Rule M3: `type + ':' + i` with `type` a
//! constant is a string that starts with `type:`; no Object.prototype
//! member's name does (ECMA-262 13.15.3, 10.1.8.1), so the read finds what
//! the program stored under a compatible key, or `undefined`. The registry
//! also holds entries under the bare type (`getDependencies(type)`).
//! The registry's `get` is a key-only parameter whose body binds a local
//! copy of it: each of its runs is one key value, so a run of `'a:' + n`
//! never joins the run of an unknown key (`reg.get(process.argv[2])`).
//! Controls: `reg.get(process.argv[2])` and `reg.get('to' + process.argv[2])`
//! may name `toString`; a store under `process.argv[2]` may be any key.
use super::*;

/// The sorted class names (or other tokens) the `.p` reads see.
fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

const CLASS_REG: &str = "class Reg { objects: any = {}; get(k: any) { return this.objects[k]; } add(k: any, o: any) { this.objects[k] = o; } }
    const reg: any = new Reg();";

const CLOSURE_REG: &str = "function Registry() { let objects: any = {};
      return { get: function (key: any) { return objects[key]; }, add: function (key: any, o: any) { objects[key] = o; },
        removeAll: function () { objects = {}; } }; }
    const reg: any = Registry();";

fn prog(regdef: &str, tail: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         {regdef}
         const n = process.argv.length;
         function make(type: any, i: any): any {{ return type === 'a' ? new A() : new B(); }}
         function dep(type: any, i: any): any {{
           const key = type + ':' + i;
           let d = reg.get(key);
           if (!d) {{ d = make(type, i); reg.add(key, d); }}
           return d;
         }}
         function deps(type: any): any {{
           let ds = reg.get(type);
           if (!ds) {{ ds = [dep(type, 0)]; reg.add(type, ds); }}
           return ds;
         }}
         dep('b', n);
         {tail}"
    )
}

fn a_only(regdef: &str, tail: &str) -> Vec<String> {
    recv(&facts(&prog(regdef, tail)))
}

fn no_member(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|r| r == "A" || r == "B")
}

#[test]
fn class_registry_prefix_read_reads_no_object_prototype_member() {
    let v = a_only(CLASS_REG, "const x: any = dep('a', n); console.log(x.p);");
    assert!(no_member(&v), "{v:?}");
}

#[test]
fn closure_registry_prefix_read_reads_no_object_prototype_member() {
    let v = a_only(CLOSURE_REG, "const x: any = dep('a', n); console.log(x.p);");
    assert!(no_member(&v), "{v:?}");
}

#[test]
fn closure_registry_beside_bare_type_keys() {
    let v = a_only(
        CLOSURE_REG,
        "deps('scene'); deps('camera'); const x: any = dep('a', n); console.log(x.p);",
    );
    assert!(no_member(&v), "{v:?}");
}

/// `dep` itself has no runs (its `type` is no switch or key-only
/// parameter): its two calls share one key `a:*|b:*`; still no member.
#[test]
fn shared_helper_with_unknown_reader_reads_no_member() {
    let v = a_only(CLASS_REG, "reg.get(process.argv[2]); const x: any = dep('a', n); console.log(x.p);");
    assert!(no_member(&v), "{v:?}");
}

#[test]
fn control_unknown_key_read_may_name_a_member() {
    let v = a_only(
        CLASS_REG,
        "reg.add('c', new A()); const x: any = reg.get(process.argv[2]); console.log(x.p);",
    );
    assert!(!no_member(&v), "control: argv key may be toString: {v:?}");
    let v = a_only(
        CLASS_REG,
        "reg.add('to' + n, new A()); const x: any = reg.get('to' + process.argv[2]); console.log(x.p);",
    );
    assert!(!no_member(&v), "control: 'to' + s may be toString: {v:?}");
}

#[test]
fn control_unknown_key_store_reaches_the_prefix_read() {
    let v = a_only(
        CLASS_REG,
        "class X { p = 3; } reg.add(process.argv[2], new X()); const x: any = dep('a', n); console.log(x.p);",
    );
    assert!(v.contains(&"A".into()) && v.contains(&"X".into()), "{v:?}");
}

fn read_only(regdef: &str, tail: &str) -> Vec<String> {
    recv(&facts(&format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         {regdef}
         const n = process.argv.length;
         function look(type: any, i: any): any {{ const key = type + ':' + i; return reg.get(key); }}
         reg.add('a:' + n, new A()); reg.add('b:' + n, new B());
         look('b', n);
         {tail}
         const x: any = look('a', n); console.log(x.p);"
    )))
}

#[test]
fn class_registry_helper_site_run_reads_its_own_prefix() {
    let v = read_only(CLASS_REG, "");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn closure_registry_helper_site_run_reads_its_own_prefix() {
    let v = read_only(CLOSURE_REG, "");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn closure_registry_bare_key_entry_stays_apart() {
    let v = read_only(CLOSURE_REG, "class S { p = 4; } reg.add('scene', new S()); reg.get('scene');");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn closure_registry_unknown_reader_stays_apart() {
    let v = read_only(CLOSURE_REG, "reg.get(process.argv[2]);");
    assert_eq!(v, vec!["A"], "{v:?}");
}

fn switch_runs(regdef: &str, tail: &str) -> Vec<String> {
    recv(&facts(&format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         {regdef}
         const n = process.argv.length;
         function look(type: any, i: any): any {{
           const key = type + ':' + i;
           let d = reg.get(key);
           if (!d) {{
             switch (type) {{
               case 'a': d = new A(); break;
               case 'b': d = new B(); break;
             }}
             reg.add(key, d);
           }}
           return d;
         }}
         look('b', n);
         {tail}
         const x: any = look('a', n); console.log(x.p);"
    )))
}

#[test]
fn class_registry_switch_run_sees_its_own_key() {
    let v = switch_runs(CLASS_REG, "reg.get(process.argv[2]);");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn closure_registry_switch_run_sees_its_own_key() {
    let v = switch_runs(CLOSURE_REG, "reg.get(process.argv[2]);");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn closure_registry_switch_run_plain() {
    let v = switch_runs(CLOSURE_REG, "");
    assert_eq!(v, vec!["A"], "{v:?}");
}

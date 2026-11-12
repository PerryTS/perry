//! A folded builtin-method node (`Expr::ArrayPush`, `Expr::SetHas`,
//! `Expr::DateSetHours`, `Expr::ArrayIndexOf`, ...; `perry_hir::folded_call`)
//! is the method call it folds: compiled code still runs an own or inherited
//! user method of that name (a data method or an accessor's result), so the
//! model resolves the call through the receiver. SOUND3/SOUND5:
//! test_parity_own_override_beats_builtin.
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_folded_call_runs_an_own_method() {
    for (name, body) in [
        (
            "own data push",
            "const a = [1]; a.push = hit; return a.push(3);",
        ),
        (
            "own accessor push",
            r#"const a = [1];
  Object.defineProperty(a, "push", { get() { return hit; } });
  return a.push(1);"#,
        ),
        (
            "own data unshift",
            "const a = [1]; a.unshift = hit; return a.unshift(3);",
        ),
        (
            "set.has",
            "const s = new Set([1]); s.has = hit; return s.has(1);",
        ),
        (
            "set.add",
            "const s = new Set(); s.add = hit; return s.add(1);",
        ),
        (
            "map.get",
            "const m = new Map(); m.get = hit; return m.get(1);",
        ),
        (
            "map.set",
            "const m = new Map(); m.set = hit; return m.set(1, 2);",
        ),
        (
            "date.setHours",
            "const d = new Date(0); d.setHours = hit; return d.setHours(3);",
        ),
        (
            "array.indexOf",
            "const a = [1, 2]; a.indexOf = hit; return a.indexOf(2);",
        ),
        (
            "array.slice",
            "const a = [3, 1]; a.slice = hit; return a.slice(0);",
        ),
        (
            "array.forEach",
            "const a = [3, 1]; a.forEach = hit; return a.forEach((x) => x);",
        ),
        (
            "array.join",
            "const a = [3, 1]; a.join = hit; return a.join(\",\");",
        ),
    ] {
        let src = format!(
            "function hit(x: any) {{ return x; }}\nfunction f() {{ {body} }}\nconsole.log(f());"
        );
        assert!(hit_runs(&src), "{name}: hit inactive, Node runs it");
    }
}

/// The native array push (`perry_hir::folded_call::native_array_push`: a
/// field receiver in value position) is that call too: codegen guards the
/// node, so an own `push` of the field's Array runs. node prints `3`.
#[test]
fn a_native_field_push_runs_an_own_method() {
    let src = r#"function hit(x: any) { return x; }
class Bag { k: number[] = [1]; add(v: number): any { return this.k.push(v); } }
const b = new Bag();
(b.k as any).push = hit;
console.log(b.add(3));"#;
    assert!(hit_runs(src), "hit inactive, Node runs it");
}

/// Control: the same push with no own method is the builtin append.
#[test]
fn a_native_field_push_without_an_own_method_stays_builtin() {
    let src = r#"function hit(x: any) { return x; }
class Bag { k: number[] = [1]; other: any = hit; add(v: number): any { return this.k.push(v); } }
const b = new Bag();
console.log(b.add(3));"#;
    assert!(!hit_runs(src), "hit runs, nothing calls it");
}

/// `Expr::ArrayPop` (a local, parameter or module-binding receiver) is the
/// call `a.pop()`: codegen calls the checked entry `js_array_m_pop0`, which
/// runs the `pop` the receiver holds. node runs `hit` in each.
#[test]
fn a_folded_pop_runs_an_own_method() {
    for (name, body) in [
        ("own data pop", "const a = [1]; (a as any).pop = hit; return a.pop();"),
        (
            "own accessor pop",
            r#"const a = [1];
  Object.defineProperty(a, "pop", { get() { return hit; } });
  return a.pop();"#,
        ),
        (
            "an object behind the annotation",
            "const o: any = { length: 0, pop: hit }; const a: number[] = o; return a.pop();",
        ),
    ] {
        let src = format!(
            "function hit(x: any) {{ return x; }}\nfunction f() {{ {body} }}\nconsole.log(f());"
        );
        assert!(hit_runs(&src), "{name}: hit inactive, Node runs it");
    }
}

/// Control: `pop` on an Array with no own `pop` is the builtin.
#[test]
fn a_folded_pop_without_an_own_method_stays_builtin() {
    let src = r#"function hit(x: any) { return x; }
function f() { const a = [1, 2]; const b: any = [3]; b.other = hit; return a.pop()! + b.pop(); }
console.log(f());"#;
    assert!(!hit_runs(src), "hit runs, nothing calls it");
}

/// Control: folded calls on receivers without an own method run the
/// builtin; a method stored under another key of another value is not
/// called by them.
#[test]
fn a_folded_call_without_an_own_method_stays_builtin() {
    let src = r#"function hit(x: any) { return x; }
function f() {
  const a = [1]; const b: any = [2]; b.other = hit;
  const s = new Set([1]); const d = new Date(0);
  a.push(3); s.add(2); d.setHours(1);
  return a.length + b.length + a.indexOf(3) + a.slice(0).length + (s.has(2) ? 1 : 0);
}
console.log(f());"#;
    assert!(!hit_runs(src), "hit runs, nothing calls it");
}

/// The builtin push keeps the element's class for a later read.
#[test]
fn a_folded_push_keeps_the_element() {
    let src = r#"class U { a = 1; }
function f() { const xs: U[] = []; xs.push(new U()); return xs[0].a; }
console.log(f());"#;
    let pf = facts(src);
    let vs: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "a")
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    assert!(
        vs.iter().any(|v| v.starts_with("T0")),
        "{vs:?}\n{}",
        pf.dump_text()
    );
}

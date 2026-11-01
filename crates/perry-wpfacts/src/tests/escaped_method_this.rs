//! DECISIONS 212 (PTH, 2026-10-27): a published method that leaves its
//! unit as a value runs on receivers its unit never saw. Found by MBW2
//! (qb9 /root/MBW2/tmp/i2x): `C.get` is published with its class, then
//! leaves through an object no contract describes (`C.box()`'s instance);
//! `main` calls it with `{ pad: 0, raw: "s" }` as `this`. Per unit the site
//! `this.raw` was proven T0 on `C` alone (the binary printed `0`), whole
//! program sees the literal too (`s`, as node).
//!
//! Two halves. A reader that holds a published method as a value (a
//! result, a field, an array element, the prototype's key) admits the
//! method's class's instances as receivers and publishes them, and reports
//! any other receiver (`Gen::stand_in_fn_of`, as a call on the class's
//! stand-in, DECISIONS 107/85). The owner lets a contract function that
//! leaves through no contract (DECISIONS 203) be called with any receiver
//! and any arguments (`Model::escape`).

use super::*;
use perry_hir::Module;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

fn run(h: &[(String, Module)], unit: bool) -> ProgramFacts {
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    match unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The read `this.<key>` in module `a.ts`.
fn read_site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    pf.sites
        .iter()
        .find(|s| s.module == mi && s.key == key && !s.is_call)
        .unwrap_or_else(|| panic!("a.ts .{key} in\n{}", pf.dump_text()))
}

fn rank(v: &crate::Verdict) -> u8 {
    match v {
        crate::Verdict::T0Mono | crate::Verdict::T0Set | crate::Verdict::OwnLength => 2,
        crate::Verdict::E0 => 1,
        crate::Verdict::No(_) => 0,
    }
}

/// Per unit claims what whole program claims of the read `this.<key>` in
/// `a.ts`: never more (the owner's rule), and not less (the receivers the
/// units publish keep it): both prove it behind the entry guard only,
/// since a receiver of another layout reaches it.
fn same_claim(mods: &[(&str, &str)], key: &str) {
    let h = hirs(mods);
    let (u, w) = (run(&h, true), run(&h, false));
    let (su, sw) = (read_site(&u, key), read_site(&w, key));
    assert_eq!(
        sw.verdict,
        crate::Verdict::E0,
        "whole program sees the other receiver:\n{}",
        w.dump_text()
    );
    assert!(
        rank(&su.verdict) <= rank(&sw.verdict),
        "per unit stronger: {:?} vs whole {:?}\nper unit:\n{}",
        su.verdict,
        sw.verdict,
        u.dump_text()
    );
    assert_eq!(
        su.verdict,
        sw.verdict,
        "per unit as whole program:\n{}",
        u.dump_text()
    );
}

const CLASS: &str = "export class C {\n    raw: any;\n    constructor() { this.raw = 1; }\n    get() { return this.raw; }\n";
const MAIN: &str = "import { C } from \"./a\";\nconst c = new C();\nconsole.log(c.get());\n";

fn class_with(statics: &str) -> String {
    format!("{CLASS}{statics}}}\n")
}

#[test]
fn a_method_returned_inside_an_undescribed_object_runs_on_any_receiver() {
    // The MBW2 program: Box's instance is described by no contract.
    let a = class_with("    static box() { function Box(this: any) { this.get = C.prototype.get; } return new (Box as any)(); }\n");
    let m =
        format!("{MAIN}const g = C.box().get;\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n");
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_stored_into_the_callers_object_runs_on_its_receivers() {
    let a = class_with("    static put(o: any) { o.get = C.prototype.get; }\n");
    let m = format!("{MAIN}const o: any = {{}};\nC.put(o);\nconsole.log(o.get.call({{ pad: 0, raw: \"s\" }}));\n");
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_returned_in_an_array_runs_on_its_receivers() {
    let a = class_with("    static arr() { return [C.prototype.get]; }\n");
    let m = format!(
        "{MAIN}const g: any = C.arr()[0];\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n"
    );
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_read_off_the_prototype_runs_on_its_receivers() {
    let a = class_with("");
    let m = format!(
        "{MAIN}const g: any = C.prototype.get;\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n"
    );
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_read_off_an_instance_runs_on_its_receivers() {
    let a = class_with("");
    let m =
        format!("{MAIN}const g: any = c.get;\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n");
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_in_a_published_literal_runs_on_its_receivers() {
    let a = format!(
        "{}export const box: any = {{ get: C.prototype.get }};\n",
        class_with("")
    );
    let m = "import { C, box } from \"./a\";\nconst c = new C();\nconsole.log(c.get());\nconsole.log(box.get.call({ pad: 0, raw: \"s\" }));\n";
    same_claim(&[("a", &a), ("main", m)], "raw");
}

#[test]
fn a_method_in_an_instance_field_runs_on_any_receiver() {
    let a = "export class C {\n    raw: any; fn: any;\n    constructor() { this.raw = 1; this.fn = C.prototype.get; }\n    get() { return this.raw; }\n}\n";
    let m = format!("{MAIN}console.log(c.fn.call({{ pad: 0, raw: \"s\" }}));\n");
    same_claim(&[("a", a), ("main", &m)], "raw");
}

#[test]
fn a_method_passed_to_another_units_function_runs_on_its_receivers() {
    // Control (holds on main): the callee's parameter contract names it.
    let a = format!(
        "import {{ run }} from \"./b\";\n{}",
        class_with("    static go() { return run(C.prototype.get); }\n")
    );
    let b = "export function run(g: any) { return g.call({ pad: 0, raw: \"s\" }); }\n";
    let m = format!("{MAIN}console.log(C.go());\n");
    same_claim(&[("b", b), ("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_stored_on_a_returned_map_runs_on_any_receiver() {
    // The Map escapes beyond any contract; what unknown code reads off it
    // does too (`Model::beyond_contract`).
    let a = class_with(
        "    static box(): any { const m: any = new Map(); m.g = C.prototype.get; return m; }\n",
    );
    let m = format!(
        "{MAIN}const g: any = C.box().g;\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n"
    );
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_stored_on_a_returned_function_runs_on_any_receiver() {
    // The arrow function leaves through its contract; its own properties
    // are read by code no contract describes.
    let a = class_with(
        "    static box(): any { const f: any = () => 1; f.g = C.prototype.get; return f; }\n",
    );
    let m = format!(
        "{MAIN}const g: any = C.box().g;\nconsole.log(g.call({{ pad: 0, raw: \"s\" }}));\n"
    );
    same_claim(&[("a", &a), ("main", &m)], "raw");
}

#[test]
fn a_method_value_run_on_its_own_class_stays_proven() {
    // The reader admits an instance of the class the owner names
    // (`FnContract::this_class`) and publishes it: nothing is lost.
    let a = class_with("");
    let m = format!("{MAIN}const g: any = C.prototype.get;\nconsole.log(g.call(new C()));\n");
    let h = hirs(&[("a", &a), ("main", &m)]);
    let (u, w) = (run(&h, true), run(&h, false));
    let (su, sw) = (read_site(&u, "raw"), read_site(&w, "raw"));
    assert_eq!(sw.verdict, crate::Verdict::T0Mono, "{}", w.dump_text());
    assert_eq!(su.verdict, sw.verdict, "per unit:\n{}", u.dump_text());
    assert!(u.broken.is_empty(), "{:?}\n{}", u.broken, u.dump_text());
}

/// Per unit never claims more than whole program of `this.<key>` in `a.ts`.
fn never_stronger(mods: &[(&str, &str)], key: &str) {
    let h = hirs(mods);
    let (u, w) = (run(&h, true), run(&h, false));
    let (su, sw) = (read_site(&u, key), read_site(&w, key));
    assert_eq!(sw.verdict, crate::Verdict::E0, "{}", w.dump_text());
    assert!(
        rank(&su.verdict) <= rank(&sw.verdict),
        "per unit stronger: {:?} vs whole {:?}\nper unit:\n{}",
        su.verdict,
        sw.verdict,
        u.dump_text()
    );
}

#[test]
fn a_method_under_a_computed_key_of_a_published_literal_runs_on_any_receiver() {
    let a = format!(
        "{}export const box: any = {{}};\nconst k = [\"g\"][0];\nbox[k] = C.prototype.get;\n",
        class_with("")
    );
    let m = "import { C, box } from \"./a\";\nconst c = new C();\nconsole.log(c.get());\nconst kk = [\"g\"][0];\nconsole.log(box[kk].call({ pad: 0, raw: \"s\" }));\n";
    never_stronger(&[("a", &a), ("main", m)], "raw");
}

#[test]
fn a_method_in_a_published_literal_the_census_broke_runs_on_any_receiver() {
    let a = format!(
        "{}export const box: any = {{ get: C.prototype.get, n: 1 }};\n",
        class_with("")
    );
    let m = "import { C, box } from \"./a\";\nconst c = new C();\nconsole.log(c.get());\ndelete box.n;\nconsole.log(box.get.call({ pad: 0, raw: \"s\" }));\n";
    same_claim(&[("a", &a), ("main", m)], "raw");
}

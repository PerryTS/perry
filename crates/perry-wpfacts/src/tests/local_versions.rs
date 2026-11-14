//! SKINROOT1: a local read right after an assignment in the same statement
//! list holds that assignment's value. A statement after `x = e;` (or
//! `let x = e;`) in the same list is reached only by falling through that
//! statement, so until the next write of `x` on the way a read of `x` sees
//! the value `e` produced, provided no other code can write `x` meanwhile
//! (no closure captures it mutably, no hoisted box holds it:
//! `narrow::scope_locals`). three.core.js `AnimationMixer._bindAction`: the
//! local `binding` is assigned `bindingsByName[ trackName ]`, then
//! `bindings[ i ]`, then `new PropertyMixer(…)`; the argument of the second
//! `_addInactiveBinding` call is only the PropertyMixer.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdicts of the call sites of `go`, in source order.
fn go_calls(src: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "go" && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("SKINROOT1 {v:?}");
    assert!(!v.is_empty(), "no call site of go");
    v
}

fn mono(v: &str) -> bool {
    v == "T0Mono" || v.starts_with("T0Mono(")
}

const CLASSES: &str = "const json: any = JSON.parse(process.argv[2] ?? \"{}\");\nclass M { go() { return 1; } }\nclass N { go() { return 2; } }\n";

/// The `_bindAction` shape: `reg` may be the `Object` constructor (an
/// unknown key read on `{}` may read `Object.prototype.constructor`), so
/// `reg[k]` may be `Object.create` and the other statics; `add`'s argument
/// is only ever the `M` just made.
#[test]
fn red_skinroot1_argument_after_the_last_assignment_is_its_value() {
    let src = format!(
        "{CLASSES}function add(reg: any, k: any, b: any) {{ b.go(); reg[k] = b; }}\n\
         function bind(reg: any, names: any, slots: any) {{ for (let i = 0; i < names.length; i++) {{ const k = names[i]; let b = reg[k]; if (b !== undefined) {{ slots[i] = b; }} else {{ b = slots[i]; if (b !== undefined) {{ continue; }} b = new M(); add(reg, k, b); slots[i] = b; }} }} }}\n\
         const root: any = ({{}} as any)[json.r];\n\
         bind(root === undefined ? {{}} : root, json.names, []);\n"
    );
    let v = go_calls(&src);
    assert!(mono(&v[0]), "add's argument is only the new M: {v:?}");
}

/// Straight line: the second assignment is what the read sees.
#[test]
fn red_skinroot1_straight_line_reassignment() {
    let v = go_calls(&format!("{CLASSES}function h() {{ let b: any = new N(); if (json.z) b.go(); b = new M(); b.go(); }}\nh();\n"));
    assert!(mono(&v[1]), "after b = new M(): {v:?}");
}

/// Control: a closure that assigns the local may run between the
/// assignment and the read: every assignment reaches it.
#[test]
fn skinroot1_control_captured_local_keeps_all_assignments() {
    let v = go_calls(&format!("{CLASSES}function h() {{ let b: any = new M(); const f = () => {{ b = new N(); }}; f(); b = new M(); f(); b.go(); }}\nh();\n"));
    assert!(!mono(&v[0]), "f may assign N after b = new M(): {v:?}");
}

/// Control: after an `if` with an assignment in each branch, both reach.
#[test]
fn skinroot1_control_join_after_if_keeps_both() {
    let v = go_calls(&format!("{CLASSES}function h() {{ let b: any; if (json.c) {{ b = new M(); }} else {{ b = new N(); }} b.go(); }}\nh();\n"));
    assert!(!mono(&v[0]), "both branches reach the read: {v:?}");
}

/// Control: a read in a loop before the loop's assignment sees the back
/// edge's value too.
#[test]
fn skinroot1_control_loop_back_edge_reaches_the_read() {
    let v = go_calls(&format!("{CLASSES}function h() {{ let b: any = new M(); for (let i = 0; i < json.n; i++) {{ b.go(); b = new N(); }} }}\nh();\n"));
    assert!(!mono(&v[0]), "the second iteration reads N: {v:?}");
}

/// SKINROOT3: the version nodes of an assignment are the body's own
/// (`Gen::narrow_assigned` owns them by the walked function, as
/// `Gen::stmt` owns a statement's nodes), so each summary run copies them
/// and one run's version holds only that run's value. three.core.js
/// `Color.setStyle`: the hoisted `function handleAlpha` is a `let` with a
/// closure; a shared version node gave every run of `setStyle` every run's
/// `handleAlpha` closure, each a new run of `handleAlpha` and of the
/// logging function it calls. Pinned by bounded counts: every version node
/// belongs to a function, and the runs of the nested function are at most
/// the runs of `setStyle`, for k = 2, 3, 4 call sites.
#[test]
fn skinroot3_versions_are_per_run_nodes() {
    for k in 2..=4 {
        let calls: String = (0..k).map(|i| format!("setStyle(json.a{i}); ")).collect();
        let src = format!(
            "const json: any = JSON.parse(process.argv[2] ?? \"{{}}\");\n\
             const out: any[] = [];\n\
             function log(...p: any[]) {{ p[0] = \"x \" + p[0]; out.push(p); }}\n\
             function setStyle(s: any) {{ function handleAlpha(x: any) {{ if (x !== undefined) log(\"alpha \" + x); }} handleAlpha(s[4]); handleAlpha(s[5]); return s; }}\n\
             for (let i = 0; i < json.n; i++) {{ {calls}}}\n"
        );
        let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
        let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
        g.register_all();
        g.link_exports();
        g.walk_all();
        g.feed_parent_values();
        let versions: Vec<usize> = (0..g.m.labels.len())
            .filter(|&n| matches!(g.m.labels[n], "assigned" | "assigned-end"))
            .collect();
        let unowned = versions.iter().filter(|&&n| g.m.node_fn[n] == crate::model::NO_FN).count();
        g.m.summarize_and_replay();
        g.m.solve();
        let runs = |name: &str| {
            g.m.sum
                .insts
                .iter()
                .filter(|i| g.m.funcs[i.f as usize].name.contains(name))
                .count()
        };
        let (outer, inner) = (runs("setStyle"), runs("handleAlpha"));
        eprintln!(
            "SKINROOT3 k={k} versions={} unowned={unowned} setStyle_runs={outer} handleAlpha_runs={inner} insts={}",
            versions.len(),
            g.m.sum.insts.len()
        );
        assert!(!versions.is_empty(), "k={k}: `let handleAlpha` makes a version");
        assert_eq!(unowned, 0, "k={k}: every version node is a node of its body");
        assert!(
            inner <= outer.max(1),
            "k={k}: one handleAlpha run per setStyle run: {inner} > {outer}"
        );
    }
}


/// SKINROOT4 hole: `var b` redeclared with an initializer in a nested
/// block writes the same local (one binding per function, ECMA-262
/// 14.3.2.1): its value reaches the read after the `if`, beside the outer
/// declaration's.
#[test]
fn red_skinroot4_nested_var_redeclaration_reaches_the_read() {
    let v = go_calls(&format!("{CLASSES}function h() {{ var b: any = new N(); if (json.z) {{ var b: any = new M(); }} b.go(); }}\nh();\n"));
    assert!(!mono(&v[0]), "the nested `var b = new M()` reaches b.go(): {v:?}");
}

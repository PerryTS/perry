//! fninfer unit tests: signatures inferred for plain JavaScript.

use super::fn_infer::{dump_module, infer_module, InferConfig};
use crate::lower_module;
use crate::Module;
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.js", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.js").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// name -> params column of the dump
fn sigs(src: &str) -> std::collections::HashMap<String, String> {
    let m = lower_src(src);
    let inf = infer_module(&m, InferConfig::default());
    let mut out = String::new();
    dump_module("test", &m, &inf, &mut out);
    if std::env::var("FNINFER_TEST_PRINT").is_ok() {
        eprintln!("{out}");
    }
    out.lines()
        .filter(|l| l.starts_with("F\t"))
        .map(|l| {
            let p: Vec<&str> = l.split('\t').collect();
            (p[4].to_string(), p[6].to_string())
        })
        .collect()
}

#[test]
fn string_methods_type_a_param() {
    let s = sigs(
        "function rootLen(path) { if (!path) return 0; const c = path.charCodeAt(0); \
         if (c === 47) return 1; const i = path.indexOf(':'); return path.length + i; }\n\
         console.log(rootLen('/a'));",
    );
    assert_eq!(s["rootLen"], "str");
}

#[test]
fn relational_compare_with_number_types_a_param() {
    let s = sigs("function synth(pos) { return !(pos >= 0); }\nconsole.log(synth(1));");
    assert_eq!(s["synth"], "f64");
}

#[test]
fn passing_to_a_typed_callee_types_a_param() {
    let s = sigs(
        "function starts(text) { return text.charCodeAt(0); }\n\
         function writeText(s) { return starts(s); }\n\
         console.log(writeText('x'));",
    );
    assert_eq!(s["writeText"], "str");
}

#[test]
fn member_names_pick_a_class() {
    let s = sigs(
        "class Pt { constructor(x, y) { this.x = x; this.y = y; } }\n\
         class Other { constructor() { this.z = 1; } }\n\
         function norm(p) { return p.x * p.x + p.y * p.y; }\n\
         console.log(norm(new Pt(1, 2)));",
    );
    assert_eq!(s["norm"], "cls:Pt");
}

#[test]
fn closures_in_a_cjs_wrapper_get_signatures() {
    let s = sigs(
        "(function () { function add(a, b) { return a + b; } \
         var total = 0; for (var i = 0; i < 3; i++) total = add(total, i); console.log(total); })();",
    );
    assert_eq!(s["add"], "f64|f64");
}

#[test]
#[ignore]
fn debug_print_hir() {
    let src = std::env::var("FNINFER_SRC").unwrap_or_default();
    let m = lower_src(&src);
    eprintln!("{:#?}", m.init);
    eprintln!("{:#?}", m.functions);
}

#[test]
fn constructor_functions_are_templates() {
    let s = sigs(
        "(function () { function Reply(res, log) { this.raw = res; this.log = log; }\n\
         Reply.prototype.send = function (p) { return this.raw + p; };\n\
         function useIt(r) { return r.raw + r.log; }\n\
         var x = new Reply(1, 2); console.log(useIt(x), x.send(3)); })();",
    );
    assert_eq!(s["useIt"], "cls:Reply");
    assert_eq!(s["Reply"], "i32|i32".replace("i32", "f64"));
}

/// `N` lines of the dump: byte offset -> callee set.
fn new_sites(src: &str) -> Vec<String> {
    let m = lower_src(src);
    let inf = infer_module(&m, InferConfig::default());
    let mut out = String::new();
    dump_module("test", &m, &inf, &mut out);
    out.lines()
        .filter(|l| l.starts_with("N\t"))
        .map(|l| l.split('\t').nth(3).unwrap_or("").to_string())
        .collect()
}

#[test]
fn callee_identity_through_a_lazy_allocator() {
    // tsc's createBaseNode: `new (C || (C = alloc.getNodeConstructor()))(…)`.
    let s = new_sites(
        "var NodeObject = class { constructor(k) { this.kind = k; } };\n\
         var alloc = { getNodeConstructor: () => NodeObject };\n\
         var C;\n\
         function make(k) { return new (C || (C = alloc.getNodeConstructor()))(k); }\n\
         console.log(make(1).kind);",
    );
    assert_eq!(s, vec!["class:NodeObject".to_string()]);
}

#[test]
fn callee_identity_unknown_for_a_parameter() {
    let s = new_sites("function make(F) { return new F(); }\nconsole.log(make(Object));");
    assert_eq!(s, vec!["unknown".to_string()]);
}

#[test]
fn a_lazy_allocator_new_types_the_factory_result() {
    // `createBaseNode`'s result is a NodeObject (class expression) or a
    // Node4 (constructor-function template), so `use(n)` sees that union.
    let s = sigs(
        "(function(){\n\
         var NodeObject = class { constructor(k) { this.kind = k; } };\n\
         function Node4(k) { this.kind = k; }\n\
         Node4.prototype.m = function () { return 1; };\n\
         var alloc = { getNodeConstructor: () => Node4 };\n\
         function setAlloc(a) { Object.assign(alloc, a); }\n\
         setAlloc({ getNodeConstructor: () => NodeObject });\n\
         let C;\n\
         function createBaseNode(k) { return new (C || (C = alloc.getNodeConstructor()))(k); }\n\
         function use(n) { return n.kind; }\n\
         console.log(use(createBaseNode(1)), new Node4(2).m());\n\
         })();",
    );
    let u = &s["use"];
    assert!(u.contains("NodeObject") && u.contains("Node4"), "{u}");
}

/// `Y`/`Z` lines of the dump.
fn layout_lines(src: &str) -> Vec<String> {
    let m = lower_src(src);
    let inf = infer_module(&m, InferConfig::default());
    let mut out = String::new();
    dump_module("test", &m, &inf, &mut out);
    out.lines()
        .filter(|l| l.starts_with("Y\t") || l.starts_with("Z\t"))
        .map(|l| l.split('\t').skip(2).collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn birth_layouts_and_family_slots() {
    // Two AST-like classes and a template: `kind` sits at 2 in all three,
    // `emitNode` does not, and a store after a call that receives `this`
    // ends the template's layout.
    let l = layout_lines(
        "(function(){\n\
         var NodeObject = class { constructor(k, p, e) { this.pos = p; this.end = e; this.kind = k; this.flags = 0; this.emitNode = void 0; } };\n\
         var TokenObject = class { constructor(k, p, e) { this.pos = p; this.end = e; this.kind = k; this.emitNode = void 0; } };\n\
         function Req(o) { this.a = o; this.b = 1; init(this); this.c = 2; }\n\
         function init(x) { x.z = 1; }\n\
         Req.prototype.m = function () { return this.b + this.a; };\n\
         function isId(node) { return node.kind === 80; }\n\
         function flags(node) { return node.emitNode; }\n\
         console.log(isId(new NodeObject(80, 0, 1)), flags(new TokenObject(1, 0, 1)), flags(new NodeObject(1, 0, 1)), new Req(3).m());\n\
         })();",
    );
    let has = |s: &str| l.iter().any(|x| x.contains(s));
    assert!(has("NodeObject 1 pos,end,kind,flags,emitNode"), "{l:?}");
    assert!(has("Req 0 a,b"), "{l:?}");
    assert!(has("p0.kind=2"), "{l:?}");
    assert!(!has("p0.emitNode="), "{l:?}");
    assert!(has("this.b=1") && has("this.a=0"), "{l:?}");
}

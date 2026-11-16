// R9BIRTH3 (2026-11-15): rule M, a method called on `this` in a
// constructor before `this` can be observed (three.js
// `this._initMemoryManager()` in AnimationMixer, Mesh's
// `this.updateMorphTargets()` reached through SkinnedMesh's `super()`).
// The callee's stores are birth stores of the allocation when the call
// resolves (ECMA-262 10.1.8 [[Get]] from the instance's prototype) to the
// constructor class's own method for every instance that runs it.
// Proven = a verdict that is neither No(..) nor Throws.
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<(u32, bool, bool, crate::Verdict)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key)
        .map(|s| (s.byte_offset, s.is_call, s.access.is_store(), s.verdict.clone()))
        .collect()
}

/// Proven by the construction-phase layout. R9STORE4: a known-object
/// store (rule A: the store adds or sets an own data property, exact for
/// any layout) is no layout claim, so these KEEP tests do not count it.
fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws | crate::Verdict::KnownObjectStore)
}

fn show(tag: &str, pf: &crate::ProgramFacts, keys: &[&str]) {
    for k in keys {
        for s in sites(pf, k) {
            eprintln!("RB3 {tag} .{k} @{} call={} store={} verdict={:?}", s.0, s.1, s.2, s.3);
        }
    }
}

/// Every store site of `key` (in a.ts) is proven / every one is not.
fn stores(pf: &crate::ProgramFacts, key: &str, want: bool) -> bool {
    let s: Vec<_> = sites(pf, key).into_iter().filter(|s| s.2).collect();
    !s.is_empty() && s.iter().all(|s| proven(&s.3) == want)
}

// M: `A`'s constructor calls its own method `init` before its last store.
// `INIT` is init's body, `TOP` runs at module level before the `new`.
fn m_src(init: &str, top: &str) -> String {
    format!(
        "function reg(x: any) {{ return x; }}
class A {{ constructor(v: any) {{ this.a = v; this.init(); this.c = 3; }}
  init() {{ {init} }}
  sum() {{ return this.a + this.b + this.c + this.d.length; }} }}
{top}const x = new A(Number(process.argv[2] ?? \"1\"));
console.log(x.sum());
"
    )
}

const M_INIT: &str = "this.b = 2; this.d = [];";

// M1: the callee's stores and the store after the call are birth stores.
#[test]
fn r9birth3_m1_direct_this_init() {
    let pf = facts(&m_src(M_INIT, ""));
    show("M1", &pf, &["a", "b", "c", "d"]);
    for k in ["a", "b", "c", "d"] {
        assert!(stores(&pf, k, true), "M1 store .{k} not proven");
    }
}

// M: the Mesh / SkinnedMesh / InstancedMesh shape. `M`'s constructor ends
// with `this.upd()`, which stores only keys `M` already holds, under an
// `if`. `S` inherits `upd`; `I` overrides it with a method that passes
// `this` out.
const S_SRC: &str = "function reg(x: any) { return x; }
class M { constructor(g: any) { this.g = g; this.inf = undefined; this.upd(); }
  upd() { if (this.g.n > 0) { this.inf = []; } } }
class S extends M { constructor(g: any) { super(g); this.isS = true; this.bm = 0; } }
class I extends M { constructor(g: any) { super(g); this.cnt = 1; } upd() { reg(this); } }
const g = { n: Number(process.argv[2] ?? \"0\") };
const s = new S(g); const i = new I(g); const m = new M(g);
console.log(s.isS, s.bm, i.cnt, m.inf, s.inf);
";

// M2: through `super()`: S's instances run M.upd (it leaks nothing), so
// S's own stores are birth stores.
#[test]
fn r9birth3_m2_through_super_inherited() {
    let pf = facts(S_SRC);
    show("M2", &pf, &["isS", "bm", "cnt"]);
    for k in ["isS", "bm"] {
        assert!(stores(&pf, k, true), "M2 store .{k} not proven");
    }
}

// M3 KEEP: I overrides `upd` and its override passes `this` out during M's
// constructor: I's own stores run after an observer saw the instance.
#[test]
fn r9birth3_m3_subclass_override_leaks() {
    let pf = facts(S_SRC);
    show("M3", &pf, &["cnt"]);
    assert!(stores(&pf, "cnt", false), "M3 store .cnt proven after I.upd passed this out");
}

// K1 KEEP: the prototype member is reassigned before the `new`.
#[test]
fn r9birth3_k1_prototype_member_reassigned() {
    let pf = facts(&m_src(M_INIT, "(A.prototype as any).init = function (this: any) { reg(this); };\n"));
    show("K1", &pf, &["b", "c"]);
    assert!(stores(&pf, "c", false), "K1 store .c proven though A.prototype.init was replaced");
}

// K2 KEEP: the callee passes `this` to an unknown function before its stores.
#[test]
fn r9birth3_k2_callee_passes_this_out() {
    let pf = facts(&m_src(&format!("reg(this); {M_INIT}"), ""));
    show("K2", &pf, &["b", "c"]);
    assert!(stores(&pf, "b", false), "K2 store .b proven after reg(this)");
    assert!(stores(&pf, "c", false), "K2 store .c proven after reg(this)");
}

// K3 KEEP: a store under `if` in the callee is no definite key.
#[test]
fn r9birth3_k3_conditional_store_in_callee() {
    let pf = facts(&m_src("if (Math.random() > 2) { this.b = 2; } this.d = [];", ""));
    show("K3", &pf, &["b", "d"]);
    assert!(stores(&pf, "b", false), "K3 store .b under if proven");
}

// K4 KEEP: the callee makes a closure over `this` and runs it (it reads
// `b`) before storing `b`.
#[test]
fn r9birth3_k4_closure_reads_before_store() {
    let pf = facts(&m_src(&format!("const f = () => this.b; reg(f()); {M_INIT}"), ""));
    show("K4", &pf, &["b", "c"]);
    assert!(stores(&pf, "b", false), "K4 store .b proven after a closure read it");
}

// K5 KEEP: `Reflect.construct(A, args, F)` makes an A instance whose
// prototype is F.prototype (10.2.2 / OrdinaryCreateFromConstructor): its
// `this.init` is F.prototype's, which passes `this` out.
#[test]
fn r9birth3_k5_reflect_construct_other_new_target() {
    let src = m_src(
        M_INIT,
        "function F(this: any) {}\nF.prototype.init = function (this: any) { reg(this); };\nconst y: any = Reflect.construct(A, [String(Math.random())], F);\nconsole.log(typeof y.c);\n",
    );
    let pf = facts(&src);
    show("K5", &pf, &["b", "c"]);
    assert!(stores(&pf, "c", false), "K5 store .c proven though a Reflect.construct instance runs F's init");
}

// K (rule K): the LoadingManager shape: closures over the alias stored
// into the instance itself, then more stores. `BODY` runs after the first
// closure store, before `this.after`.
fn k_src(body: &str, member: &str) -> String {
    format!(
        "function reg(x: any) {{ return x; }}
class LM {{ constructor(onLoad: any) {{
    const scope = this;
    let total = 0;
    this.onLoad = onLoad;
    this.itemEnd = function (url: string) {{ total++; if (scope.onLoad !== undefined) scope.onLoad(url, total); }};
{body}    this.after = 1;
    this.itemStart = function (url: string) {{ total++; return scope.after; }};
    this.last = 2;
  }}
{member} }}
const lm = new LM((u: string, n: number) => console.log(u, n));
lm.itemEnd(String(process.argv[2] ?? \"a\"));
console.log(lm.after + lm.last, lm.itemStart(\"b\"));
"
    )
}

// C1: the stores after each closure store are birth stores.
#[test]
fn r9birth3_c1_closure_stored_into_this() {
    let pf = facts(&k_src("", ""));
    show("C1", &pf, &["itemEnd", "after", "itemStart", "last"]);
    for k in ["itemEnd", "after", "itemStart", "last"] {
        assert!(stores(&pf, k, true), "C1 store .{k} not proven");
    }
}

// C2 KEEP: the closure is called in the constructor before `this.after`.
#[test]
fn r9birth3_c2_closure_called_in_ctor() {
    let pf = facts(&k_src("    this.itemEnd(\"x\");\n", ""));
    show("C2", &pf, &["after", "last"]);
    assert!(stores(&pf, "after", false), "C2 store .after proven after the closure ran");
}

// C3 KEEP: the closure is passed to an unknown function.
#[test]
fn r9birth3_c3_closure_passed_out() {
    let pf = facts(&k_src("    reg(this.itemEnd);\n", ""));
    show("C3", &pf, &["after", "last"]);
    assert!(stores(&pf, "after", false), "C3 store .after proven after the closure was passed out");
}

// C4 KEEP: a getter of the class hands the closure out, read in the
// constructor.
#[test]
fn r9birth3_c4_getter_returns_closure() {
    let pf = facts(&k_src("    reg(this.h);\n", "  get h() { return this.itemEnd; }"));
    show("C4", &pf, &["after", "last"]);
    assert!(stores(&pf, "after", false), "C4 store .after proven after a getter handed the closure out");
}

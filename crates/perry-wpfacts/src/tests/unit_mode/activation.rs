//! DECISIONS 247: per unit a function body contributes its births,
//! effects, escapes and publications only once the program can run it
//! (DECISIONS 225's activation, per unit): a module body, a solved target
//! of an active body, a function another unit runs (`Published::method_runs`
//! / `ctor_runs`), one that escapes from active code (in the unit, or a
//! broken `fn:` / class display across units), and conservatively the
//! accessors and protocol methods of the unit's classes. TypeScript: dead
//! server/ code handed the `ts` namespace to unknown code per unit only,
//! and with it the `Diagnostics` map (2,995 reads).

use super::*;

/// `diagnosticInformationMap.generated.ts`: a literal of `diag()` records.
const A: &str = r#"function diag(code: number, category: number, key: string, message: string) {
  return { code, category, key, message };
}
export const Diagnostics = {
  Alpha_0: diag(1001, 1, "Alpha_0_1001", "Alpha {0}"),
  Beta_1: diag(1002, 0, "Beta_1_1002", "Beta {1}"),
};"#;

/// `binder.ts`: the records read in a loop.
const C: &str = r#"import { Diagnostics } from "./a";
export function bind(n: number) {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const d = i % 2 ? Diagnostics.Alpha_0 : Diagnostics.Beta_1;
    s += d.code + d.message.length;
  }
  return s;
}"#;

/// `server/project.ts`: `Project.enableProxy` hands the namespace to a
/// parameter function.
const P: &str = r#"import * as ts from "./a";
export class P {
  n = 0;
  m(f: (m: { typescript: typeof ts }) => unknown) {
    f({ typescript: ts });
    this.n++;
  }
}"#;

/// Whether every read of the records in `c.ts` is proven (and there are
/// some).
fn diag_reads_proven(pf: &ProgramFacts) -> bool {
    let keys = ["Alpha_0", "Beta_1", "code", "message"];
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| pf.module_paths[s.module as usize] == "c.ts" && keys.contains(&s.key.as_str()))
        .collect();
    assert!(
        !sites.is_empty(),
        "no record read in c.ts:\n{}",
        pf.dump_text()
    );
    sites
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// Whether the census broke a layout of `a.ts` (the records or the map):
/// per unit, what reaches unknown code breaks its display. (Per unit the
/// record reads of another unit's map are not proven even without any
/// escape, a separate gap: the census is the mechanism here.)
fn diag_broken(pf: &ProgramFacts) -> bool {
    pf.broken.iter().any(|d| d.starts_with("a.ts:"))
}

fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut m = vec![("a", A), ("c", C), ("p", P)];
    m.extend_from_slice(extra);
    m
}

fn sabotaged(path: &str) -> FactsConfig {
    FactsConfig {
        sabotage: Some(format!("activation:{path}")),
        ..FactsConfig::default()
    }
}

/// (a) fx5e: a dead function (exported, never called) hands the namespace
/// to unknown code. Whole never walks it; per unit it is not active either,
/// so the records stay proven in both modes.
#[test]
fn dead_escape_breaks_nothing() {
    let q = r#"import * as ts from "./a";
export function dead() { const g = (globalThis as any).hook; g({ typescript: ts }); }"#;
    let main = r#"import { bind } from "./c";
import { P } from "./p";
import { dead } from "./q";
console.log(bind(4), typeof P, typeof dead);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        !diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
}

/// (b) fx5f, the tssrc chain: dead code lets a `P` reach unknown code; per
/// unit before DECISIONS 247 that broke `P`, `P::m` ran with an unknown
/// argument and escaped the namespace. Dead code runs nothing.
#[test]
fn dead_class_escape_runs_no_method() {
    let q = r#"import { P } from "./p";
export function dead() { const g = (globalThis as any).hook; g(new P()); }"#;
    let main = r#"import { bind } from "./c";
import { P } from "./p";
import { dead } from "./q";
console.log(bind(4), typeof P, typeof dead);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        !diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    assert!(
        !u.broken
            .iter()
            .any(|d| d.ends_with(":P") || d.contains("P::m")),
        "{:?}",
        u.broken
    );
}

/// (c) Soundness: a function reached only through an escaped value (its
/// module body hands it to unknown code) is active, so the namespace it
/// hands on escapes in both modes; with escape activation left out, per
/// unit proves the records wrongly.
#[test]
fn escaped_function_is_active() {
    let q = r#"import * as ts from "./a";
function leak() { const g = (globalThis as any).hook2; g({ typescript: ts }); }
(globalThis as any).hook = leak;
export const k = 1;"#;
    let main = r#"import { bind } from "./c";
import { k } from "./q";
console.log(bind(4), k);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    let s = units_with(&mods, &sabotaged("runs"));
    assert!(
        !diag_broken(&s),
        "sabotaged runs activation still breaks {:?}",
        s.broken
    );
}

/// (d) Cross-unit: `main`'s module body calls `q.f`, which hands the
/// namespace to unknown code. `f` is active in its unit through the joined
/// runs (`Published::method_runs`), so the records escape in both modes;
/// without the runs channel per unit proves them wrongly.
#[test]
fn function_another_unit_runs_is_active() {
    let q = r#"import * as ts from "./a";
export function f() { const g = (globalThis as any).hook; g({ typescript: ts }); }"#;
    let main = r#"import { bind } from "./c";
import { f } from "./q";
f();
console.log(bind(4));"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    let s = units_with(&mods, &sabotaged("runs"));
    assert!(
        !diag_broken(&s),
        "sabotaged runs activation still breaks {:?}",
        s.broken
    );
}

/// (e) Guard: an accessor of a published class runs on another unit's
/// read, with no run published. The instance this unit makes for the other
/// unit's `new Q()` leaves through its contract, and leaving walks every
/// getter on its chain (`Model::accessor_walk` with `Key::Any`, the
/// `accessor` activation of `group_connect`): DECISIONS 247 (5) is that
/// walk. The getter is active and its escape seen in both modes; with the
/// accessor activation sabotaged, per unit proves the records wrongly.
#[test]
fn published_accessor_is_active() {
    let q = r#"import * as ts from "./a";
export class Q { get t() { const g = (globalThis as any).hook; g({ typescript: ts }); return 1; } }"#;
    let main = r#"import { bind } from "./c";
import { Q } from "./q";
console.log(bind(4), new Q().t);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    let s = units_with(&mods, &sabotaged("accessor"));
    assert!(
        !diag_broken(&s),
        "sabotaged accessor activation still breaks {:?}",
        s.broken
    );
}

/// (e) Guard: a protocol method (`toString`) of a published class runs on
/// another unit's conversion. Active through the protocol call the
/// instance's leaving makes (the `call` activation, DECISIONS 247 (5)),
/// and through `main`'s conversion, a run it publishes (`runs`, (3)).
#[test]
fn published_protocol_method_is_active() {
    let q = r#"import * as ts from "./a";
export class Q { toString() { const g = (globalThis as any).hook; g({ typescript: ts }); return "q"; } }"#;
    let main = r#"import { bind } from "./c";
import { Q } from "./q";
console.log(bind(4), `${new Q()}`);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    // `main`'s published run holds it alone (the protocol call needs the
    // instance, which exists once `main`'s construction runs the
    // constructor, a run as well): without runs nothing activates it.
    let s = units_with(&mods, &sabotaged("call"));
    assert!(
        diag_broken(&s),
        "call sabotaged: the runs activation did not hold {:?}",
        s.broken
    );
    let s = units_with(&mods, &sabotaged("runs"));
    assert!(
        !diag_broken(&s),
        "sabotaged runs activation still breaks {:?}",
        s.broken
    );
}

/// The function of (c), (c2)-(c5): it hands the namespace to unknown code.
const LEAK: &str =
    "function leak() { const g = (globalThis as any).hook2; g({ typescript: ts }); }";

/// (c)-(c5) per unit: `leak` is active through `path` alone: the records
/// break in both modes, and with `path` sabotaged they stay proven per unit.
fn escapes_through(q: &str, main: &str, path: &str) {
    let q = format!("import * as ts from \"./a\";\n{LEAK}\n{q}");
    let q: &'static str = Box::leak(q.into_boxed_str());
    let main: &'static str = Box::leak(main.to_string().into_boxed_str());
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    let s = units_with(&mods, &sabotaged(path));
    assert!(
        !diag_broken(&s),
        "sabotaged {path} activation still breaks {:?}",
        s.broken
    );
}

/// (c2) A function passed to an unknown call from active code.
#[test]
fn function_passed_to_unknown_call_is_active() {
    escapes_through(
        "(globalThis as any).register(leak);\nexport const k = 1;",
        "import { bind } from \"./c\";\nimport { k } from \"./q\";\nconsole.log(bind(4), k);",
        "runs",
    );
}

/// (c3) A function stored into a program-owned global (a contract cell
/// per unit) that another unit reads and calls. (Nothing reads a
/// program-owned global the program does not read: closed world, and the
/// host defines no such key.)
#[test]
fn function_stored_into_global_cell_is_active() {
    escapes_through(
        "(globalThis as any).__hook = leak;\nexport const k = 1;",
        "import { bind } from \"./c\";\nimport { k } from \"./q\";\n(globalThis as any).__hook();\nconsole.log(bind(4), k);",
        "runs",
    );
}

/// (c4) A function stored into a published holder, which another unit
/// calls: the run comes back from the other unit.
#[test]
fn function_in_published_holder_another_unit_calls_is_active() {
    escapes_through(
        "export const o: any = {};\no.k = leak;",
        "import { bind } from \"./c\";\nimport { o } from \"./q\";\no.k();\nconsole.log(bind(4));",
        "runs",
    );
}

/// (c5) A function returned from an active exported function, which
/// another unit calls.
#[test]
fn function_returned_from_exported_function_is_active() {
    escapes_through(
        "export function mk() { return leak; }",
        "import { bind } from \"./c\";\nimport { mk } from \"./q\";\nmk()();\nconsole.log(bind(4));",
        "runs",
    );
}

/// DECISIONS 247: a publication is no escape. An exported function no unit
/// runs stays inactive per unit (as in the whole program): the namespace
/// it would hand on stays proven in both modes.
#[test]
fn exported_function_never_run_is_inactive() {
    let q = r#"import * as ts from "./a";
export function f() { const g = (globalThis as any).hook; g({ typescript: ts }); }
export function h(x: () => void) { return x; }"#;
    let main = r#"import { bind } from "./c";
import { f, h } from "./q";
console.log(bind(4), h(f) === f);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        !diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
}

/// DECISIONS 247: a published class's instance another unit holds runs no
/// plain method of it unless that unit runs it (a published run): the
/// method stays inactive per unit, as in the whole program.
#[test]
fn published_instance_runs_no_plain_method() {
    let q = r#"import * as ts from "./a";
export class Q { m() { const g = (globalThis as any).hook; g({ typescript: ts }); } }"#;
    let main = r#"import { bind } from "./c";
import { Q } from "./q";
console.log(bind(4), new Q() !== null);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(
        diag_reads_proven(&w),
        "whole:
{}",
        w.dump_text()
    );
    assert!(
        !diag_broken(&u),
        "per unit: {:?}
{}",
        u.broken,
        u.dump_text()
    );
}

/// tssrc: every published class's constructor ran per unit, through the
/// construction standing for the other units' `new` (`publish_class`), so
/// server/'s `Session` built a `ProjectService` that built every `Project`
/// (and broke it). That construction is no run: a constructor no unit
/// constructs is dead per unit as in whole.
const QC: &str = r#"import * as ts from "./a";
export class Q {
  n = 0;
  constructor() { const g = (globalThis as any).hook; g({ typescript: ts }); }
}"#;

/// A field initializer handing the namespace out.
const QF: &str = r#"import * as ts from "./a";
export class Q { h = (globalThis as any).hook({ typescript: ts }); }"#;

#[test]
fn constructor_no_unit_constructs_is_inactive() {
    for q in [QC, QF] {
        let main = r#"import { bind } from "./c";
import { Q } from "./q";
console.log(bind(4), typeof Q);"#;
        let mods = with(&[("q", q), ("main", main)]);
        let (u, w) = (units_of(&mods), whole_of(&mods));
        assert!(diag_reads_proven(&w), "whole {q}:\n{}", w.dump_text());
        assert!(
            !diag_broken(&u),
            "per unit {q}: {:?}\n{}",
            u.broken,
            u.dump_text()
        );
    }
}

/// Soundness: another unit's `new Q()` runs `Q`'s constructor and field
/// initializers (`new <display>`, published by `main`): their escape is
/// seen in both modes; with the runs activation sabotaged it is not (the
/// instance stays in `main`: no escape breaks `Q`).
#[test]
fn constructor_another_unit_constructs_is_active() {
    for q in [QC, QF] {
        let main = r#"import { bind } from "./c";
import { Q } from "./q";
const q = new Q();
console.log(bind(4), typeof q);"#;
        let mods = with(&[("q", q), ("main", main)]);
        let (u, w) = (units_of(&mods), whole_of(&mods));
        assert!(!diag_reads_proven(&w), "whole {q}:\n{}", w.dump_text());
        assert!(
            diag_broken(&u),
            "per unit {q}: {:?}\n{}",
            u.broken,
            u.dump_text()
        );
        let s = units_with(&mods, &sabotaged("runs"));
        assert!(
            !diag_broken(&s),
            "{q}: sabotaged runs activation still breaks {:?}",
            s.broken
        );
    }
}

/// Soundness: a subclass of another unit with no constructor of its own,
/// constructed in a third: the chain runs `Q`'s constructor (the
/// constructing unit publishes `new` of each class of the chain).
#[test]
fn parent_constructor_of_a_constructed_subclass_is_active() {
    for q in [QC, QF] {
        let r = r#"import { Q } from "./q";
export class R extends Q { k = 1; }"#;
        let main = r#"import { bind } from "./c";
import { R } from "./r";
console.log(bind(4), new R().k);"#;
        let mods = with(&[("q", q), ("r", r), ("main", main)]);
        let (u, w) = (units_of(&mods), whole_of(&mods));
        assert!(!diag_reads_proven(&w), "whole {q}:\n{}", w.dump_text());
        assert!(
            diag_broken(&u),
            "per unit {q}: {:?}\n{}",
            u.broken,
            u.dump_text()
        );
    }
}

/// The owner of `Ball` runs its constructor only once `m` publishes its
/// `new` (DECISIONS 247): until then `x` holds no value in its contract.
const BALL: &str = r#"export class Ball { x: number; constructor(r: number) { this.x = r; } bounce() { this.x += 1; return this.x; } }"#;

/// DECISIONS 247: `m` constructs `Ball` of another unit. The owner runs
/// the constructor once `m` publishes the `new`; until then the field the
/// constructor stores holds no value in the published contract: a reader
/// takes nothing there (`units::plain_prims`), never ⊤. Per unit `x` is
/// proven and `Ball` is unbroken, as whole (with the empty kind read as ⊤,
/// `m` reported `Ball` and the report stuck).
#[test]
fn constructor_dependent_layout_reads_nothing_before_the_run() {
    let mods = [
        ("a", BALL),
        ("m", "import { Ball } from './a'; console.log(new Ball(1));"),
    ];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&w, "x"), "whole:\n{}", w.dump_text());
    assert!(proven(&u, "x"), "per unit:\n{}", u.dump_text());
    assert!(!u.broken.iter().any(|d| d == "a.ts:Ball"), "{:?}", u.broken);
}

/// DECISIONS 247 (5): the static parts of a class's contract (its static
/// methods and data) hold whether or not its constructor ran: `m` calls
/// `K.make()` of a class nobody constructs. The static method runs (its
/// read of `K.base` is proven in both modes) and the constructor stays
/// inactive (its store of `y` is walked in neither mode).
#[test]
fn static_parts_publish_at_once() {
    let a = r#"export class K { y: number; static base = 2; constructor() { this.y = 3; } static make(): number { return K.base + 1; } }"#;
    let mods = [
        ("a", a),
        ("m", "import { K } from './a'; console.log(K.make());"),
    ];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&w, "base"), "whole:\n{}", w.dump_text());
    assert!(proven(&u, "base"), "per unit:\n{}", u.dump_text());
    assert!(
        verdicts(&w, "y").is_empty(),
        "whole walks the constructor:\n{}",
        w.dump_text()
    );
    assert!(
        verdicts(&u, "y").is_empty(),
        "per unit walks the constructor:\n{}",
        u.dump_text()
    );
    assert!(u.broken.is_empty(), "{:?}", u.broken);
}

/// DECISIONS 247: `new C` of a class whose constructor-less body extends
/// a class of the same unit runs the parent's constructor: the other
/// unit names `C` alone (its contract holds the chain inside the owner),
/// so the owner activates the chain's constructors for it. The namespace
/// `B`'s constructor hands to unknown code is seen in both modes; with the runs activation sabotaged it is
/// not.
#[test]
fn parent_constructor_inside_the_owner_runs_for_a_subclass_new() {
    let q = r#"import * as ts from "./a";
class B { constructor() { const g = (globalThis as any).hook; g({ typescript: ts }); } }
export class C extends B {}"#;
    let main = r#"import { bind } from "./c";
import { C } from "./q";
const c = new C();
console.log(bind(4), typeof c);"#;
    let mods = with(&[("q", q), ("main", main)]);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    let s = units_with(&mods, &sabotaged("runs"));
    assert!(
        !diag_broken(&s),
        "sabotaged runs activation still breaks {:?}",
        s.broken
    );
}

/// DECISIONS 247: a field of a built class that no value reached yet holds
/// nothing for a reader in another unit, never ⊤. `m` constructs `Box`
/// with an `Item` before the owner has run either constructor, so the
/// contract `m` first reads publishes `v` with an empty kind. `m` calls
/// through `b.v`: it takes nothing from the contract (its
/// own `Item` arrives once the owner stores it), so `b` does not escape
/// and `Box` is unbroken, as whole. With `v` read as ⊤ the call has an
/// unknown callee, `b` escapes, and the report sticks.
#[test]
fn a_field_no_value_reached_yet_reads_nothing() {
    let a = r#"export class Item { n = 1; ping(b: Box) { return b.k; } }
export class Box { v: Item; k = 2; constructor(v: Item) { this.v = v; } }"#;
    let mods = [
        ("a", a),
        ("m", "import { Box, Item } from './a'; const b = new Box(new Item()); console.log(b.v.ping(b), b.k);"),
    ];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&w, "k"), "whole:\n{}", w.dump_text());
    assert!(
        !u.broken.iter().any(|d| d == "a.ts:Box"),
        "{:?}\n{}",
        u.broken,
        u.dump_text()
    );
    assert!(proven(&u, "k"), "per unit:\n{}", u.dump_text());
}

/// The modules of (c)-(c5) with a unit `f` between `q` and `main`: `leak`
/// runs, so the records break in both modes; with `path` sabotaged (when
/// given) they stay proven per unit.
fn escapes_through_via(q: &str, f: &'static str, main: &'static str, path: Option<&str>) {
    let q = format!("import * as ts from \"./a\";\n{LEAK}\n{q}");
    let q: &'static str = Box::leak(q.into_boxed_str());
    let mut extra = vec![("q", q), ("main", main)];
    if !f.is_empty() {
        extra.push(("f", f));
    }
    let mods = with(&extra);
    let (u, w) = (units_of(&mods), whole_of(&mods));
    assert!(!diag_reads_proven(&w), "whole:\n{}", w.dump_text());
    assert!(
        diag_broken(&u),
        "per unit: {:?}\n{}",
        u.broken,
        u.dump_text()
    );
    if let Some(path) = path {
        let s = units_with(&mods, &sabotaged(path));
        assert!(
            !diag_broken(&s),
            "sabotaged {path} activation still breaks {:?}",
            s.broken
        );
    }
}

/// tsc's enum spelling: the table's keys are computed.
const ENUM_K: &str = "var K: any; (function (K: any) { K[K[\"a\"] = 0] = \"a\"; K[K[\"b\"] = 1] = \"b\"; })(K || (K = {}));\n";
const TABLE_FNS: &str =
    "function fa(n: any) { return n.x; }\nfunction fb(n: any) { leak(); return 0; }\n";
const WALK: &str = "export function walk(node: any) { const fn = table[node.kind]; return fn === undefined ? undefined : fn(node); }";

/// tscwork `forEachChild`: `const fn = forEachChildTable[node.kind]; fn(…)`
/// over a literal whose keys are computed (`[SyntaxKind.X]: f`). Whole
/// calls the members through the computed read; per unit a reader reads a
/// computed key of the literal as unknown (no contract describes it), so
/// what it holds there leaves beyond any contract and its functions are
/// active (DECISIONS 203/247). Before, they left as a publication: no
/// member ran per unit (330 tscwork sites absent).
#[test]
fn function_under_a_computed_key_of_a_published_literal_is_active() {
    let q = format!("{ENUM_K}{TABLE_FNS}export const table: any = {{ [K.a]: fa, [K.b]: fb }};");
    let f = format!("import {{ table }} from \"./q\";\n{WALK}");
    let main = "import { bind } from \"./c\";\nimport { walk } from \"./f\";\nwalk({ kind: process.argv.length, x: 1 });\nconsole.log(bind(4));";
    escapes_through_via(&q, Box::leak(f.into_boxed_str()), main, None);
    // Methods under computed keys.
    let q = format!("{ENUM_K}export const table: any = {{ [K.a](n: any) {{ return n.x; }}, [K.b](n: any) {{ leak(); return 0; }} }};");
    let f = format!("import {{ table }} from \"./q\";\n{WALK}");
    escapes_through_via(&q, Box::leak(f.into_boxed_str()), main, None);
}

/// The same with table and walker in one module (two fact units of it, as
/// typescript.js's `forEachChildTable` and `forEachChild`).
#[test]
fn function_under_a_computed_key_read_by_another_unit_of_the_module_is_active() {
    let q = format!("{ENUM_K}{TABLE_FNS}const table: any = {{ [K.a]: fa, [K.b]: fb }};\n{WALK}");
    let main = "import { bind } from \"./c\";\nimport { walk } from \"./q\";\nwalk({ kind: process.argv.length, x: 1 });\nconsole.log(bind(4));";
    escapes_through_via(&q, "", main, None);
}

/// Guards (green before and after): a published literal (module binding,
/// a function's result, closed or open shape) that is the receiver of a
/// call of an unknown function value, or passed to one, escapes per unit
/// as in whole and its members run (escape and runs both activate them).
#[test]
fn published_literal_reaching_unknown_code_runs_its_members() {
    let run_recv = "import { sys } from \"./q\";\nexport function run() { sys.ext(\"x\"); }";
    let main = "import { bind } from \"./c\";\nimport { run } from \"./f\";\nrun();\nconsole.log(bind(4));";
    for q in [
        "export const sys: any = { readFile(p: any) { return p; }, write(s: any) { leak(); }, ext: (globalThis as any).ext };",
        "function getSys() { function rd(p: any) { leak(); } const o = { readFile(p: any) { return p; }, rd, ext: (globalThis as any).ext }; return o; }\nexport var sys: any = (() => getSys())();",
        "function getSys() { const o = { get dbg() { return 1; }, readFile(p: any) { return p; }, rd: (p: any) => { leak(); }, ext: (globalThis as any).ext }; return o; }\nexport var sys: any = getSys();",
    ] {
        escapes_through_via(q, run_recv, main, None);
    }
    escapes_through_via(
        "export const sys: any = { readFile(p: any) { return p; }, write(s: any) { leak(); } };",
        "import { sys } from \"./q\";\nexport function run(h: any) { h(sys); }",
        "import { bind } from \"./c\";\nimport { run } from \"./f\";\nrun((globalThis as any).ext);\nconsole.log(bind(4));",
        None,
    );
}

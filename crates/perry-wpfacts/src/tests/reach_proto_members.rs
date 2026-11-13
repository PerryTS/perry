//! Activation reach of a member the program puts on a built-in prototype
//! (class H of the activation false proofs,
//! temporal/test_gap_temporal_keyed_read_chain).
//!
//! Held keys: a constructor a host object holds (`Temporal.Duration`,
//! `Intl.NumberFormat`) is a native function value, and the realm table
//! listed the own keys of such a function for node modules only. Its
//! `prototype` read as absent, so `F.prototype.m = f` and
//! `Object.defineProperty(F.prototype, ...)` had no receiver: `f` went
//! nowhere and was called inactive while a read on an instance ran it. The
//! table's `held` rows declare those keys; the read is the opaque member,
//! as for a node module's class.
//!
//! Prototype accessors: an accessor on `ObjectProto` (every built-in
//! prototype) ran only where a lookup walks: on a token. A primitive's
//! lookup and an unknown value's walk nothing, so the getter of
//! `Object.defineProperty(String.prototype, k, { get })` was inactive for
//! `"s"[k]`. The accessor now escapes.
//!
//! Each program's marker function `hit` runs only through the member under
//! test; the control is the same solve without the rule (`noreachheldkeys`,
//! `noreachprotoaccessors`: what `PERRY_FACTS_REACH_HELD_KEYS=0` and
//! `PERRY_FACTS_REACH_PROTO_ACCESSORS=0` give).

use super::*;

fn hit_runs(src: &str, sabotage: Option<&str>) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let cfg = FactsConfig {
        sabotage: sabotage.map(str::to_string),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const HEAD: &str = "function hit(): any { return 1; }\n";

/// A member put on the prototype of a constructor a host object holds:
/// inactive without the `held` rows.
const HELD: &[(&str, &str)] = &[
    (
        "a method on Temporal.Duration.prototype, read by key",
        "function read(o: any, k: string): any { return o[k]; }\n\
         const d: any = new Temporal.Duration(1, 2, 3, 4);\n\
         (Temporal.Duration.prototype as any).describeYears = function (this: any) { hit(); return this.years; };\n\
         console.log(read(d, \"describeYears\").call(d));",
    ),
    (
        "a method on Temporal.PlainDate.prototype, called",
        "const d: any = new Temporal.PlainDate(2020, 5, 6);\n\
         (Temporal.PlainDate.prototype as any).tag = function (this: any) { hit(); return this.day; };\n\
         console.log(d.tag());",
    ),
    (
        "a getter on Temporal.Duration.prototype",
        "const d: any = new Temporal.Duration(1, 2, 3, 4);\n\
         Object.defineProperty(Temporal.Duration.prototype, \"twiceDays\", { get(this: any) { hit(); return this.days * 2; }, configurable: true });\n\
         console.log(d.twiceDays);",
    ),
    (
        "a method on Intl.NumberFormat.prototype",
        "const f: any = new Intl.NumberFormat(\"en-US\");\n\
         (Intl.NumberFormat.prototype as any).zed = function (this: any) { hit(); return this.format(1); };\n\
         console.log(f.zed());",
    ),
    (
        "a getter on Intl.DateTimeFormat.prototype",
        "const f: any = new Intl.DateTimeFormat(\"en-US\");\n\
         Object.defineProperty(Intl.DateTimeFormat.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         const k: string = \"zed\";\n\
         console.log(f[k]);",
    ),
    (
        "a getter of an argument of a held constructor's static",
        "const d: any = Temporal.PlainDate.from({ year: 2020, month: 1, get day() { hit(); return 2; } } as any);\n\
         console.log(d.day);",
    ),
    (
        "the same static read as a value first",
        "const from: any = Temporal.PlainDate.from;\n\
         const d = from.call(Temporal.PlainDate, { year: 2020, month: 1, get day() { hit(); return 2; } });\n\
         console.log(d.day);",
    ),
    (
        "the prototype held in a program object first",
        "const reg: any = { p: Temporal.PlainTime.prototype };\n\
         reg.p.hh = function (this: any) { hit(); return this.hour; };\n\
         const t: any = new Temporal.PlainTime(5, 6);\n\
         console.log(t.hh());",
    ),
];

/// An accessor on a built-in prototype reached by a lookup the solve does
/// not walk: inactive without the escape.
const ACCESSORS: &[(&str, &str)] = &[
    (
        "a getter on String.prototype, a string receiver",
        "Object.defineProperty(String.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         const s: any = \"s\";\n\
         console.log(s.zed);",
    ),
    (
        "a getter on Number.prototype, read by key",
        "function read(o: any, k: string): any { return o[k]; }\n\
         Object.defineProperty(Number.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         console.log(read(5, \"zed\"));",
    ),
    (
        "a getter on Boolean.prototype",
        "Object.defineProperty(Boolean.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         const b: any = true;\n\
         console.log(b.zed);",
    ),
    (
        "a setter on String.prototype, a string receiver",
        "Object.defineProperty(String.prototype, \"wr\", { set(this: any, v: any) { hit(); }, configurable: true });\n\
         const s: any = \"s\";\n\
         s.wr = 1;",
    ),
    (
        "an index getter on String.prototype, a string index read past the end",
        "function g(t: string, k: number): any { return t[k]; }\n\
         Object.defineProperty(String.prototype, \"5\", { get(this: any) { hit(); return 7; }, configurable: true });\n\
         let out = \"\";\n\
         for (let k = 0; k < 7; k++) out += g(\"abc\", k) + g(\"xy\", k) + \";\";\n\
         console.log(out);",
    ),
    (
        "an index getter on String.prototype, an untyped receiver",
        "function g(t: any, k: number): any { return t[k]; }\n\
         Object.defineProperty(String.prototype, \"5\", { get(this: any) { hit(); return 7; }, configurable: true });\n\
         let out = \"\";\n\
         for (let k = 0; k < 7; k++) out += g(\"abc\", k) + g(\"xy\", k) + \";\";\n\
         console.log(out);",
    ),
    (
        "a getter on Date.prototype, an unknown receiver",
        "Object.defineProperty(Date.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         const u: any = (globalThis as any).someHostValue;\n\
         if (u) console.log(u.zed);",
    ),
];

/// What already reached the member: a constructor the global object holds
/// (a namespace of its own), and a receiver whose lookup is walked.
const COVERED: &[(&str, &str)] = &[
    (
        "a method on Date.prototype",
        "const d: any = new Date(0);\n\
         (Date.prototype as any).zed = function (this: any) { hit(); return this.getTime(); };\n\
         console.log(d.zed());",
    ),
    (
        "a method on String.prototype, a string receiver",
        "(String.prototype as any).zed = function (this: any) { hit(); return 1; };\n\
         const s: any = \"s\";\n\
         console.log(s.zed());",
    ),
    (
        "a getter on Map.prototype, a Map receiver",
        "Object.defineProperty(Map.prototype, \"zed\", { get(this: any) { hit(); return 1; }, configurable: true });\n\
         const m: any = new Map();\n\
         console.log(m.zed);",
    ),
    (
        "a method on a node module's class prototype",
        "import * as stream from \"node:stream\";\n\
         ((stream as any).Readable.prototype as any).zed = function (this: any) { hit(); return 1; };\n\
         const r: any = new (stream as any).Readable({ read() {} });\n\
         console.log(r.zed());",
    ),
];

/// A function nothing stores or installs stays inactive under both rules.
const NEVER: &[(&str, &str)] = &[
    (
        "another member on the prototype",
        "(Temporal.Duration.prototype as any).m = function (this: any) { return this.years; };\n\
         const d: any = new Temporal.Duration(1);\n\
         console.log(d.m(), typeof hit);",
    ),
    (
        "a data property defined on String.prototype",
        "Object.defineProperty(String.prototype, \"zed\", { value: 1, configurable: true });\n\
         const s: any = \"s\";\n\
         console.log(s.zed, typeof hit);",
    ),
    (
        "the statics and the prototype only read",
        "const P: any = Temporal.Duration.prototype;\n\
         const f: any = Temporal.PlainDate.from;\n\
         console.log(typeof P, typeof f, typeof Intl.NumberFormat.supportedLocalesOf, typeof hit);",
    ),
];

fn holes(list: &[(&str, &str)], sabotage: &str) {
    for (what, body) in list {
        let src = format!("{HEAD}{body}");
        assert!(
            hit_runs(&src, None),
            "{what}: hit inactive with the rule\n{src}"
        );
        assert!(
            !hit_runs(&src, Some(sabotage)),
            "{what}: hit active without the rule (no hole to close)\n{src}"
        );
    }
}

#[test]
fn a_member_on_a_held_constructors_prototype_is_reached() {
    holes(HELD, "noreachheldkeys");
}

#[test]
fn an_accessor_on_a_builtin_prototype_runs_for_an_unwalked_receiver() {
    holes(ACCESSORS, "noreachprotoaccessors");
}

#[test]
fn members_reached_before_stay_reached() {
    for (what, body) in COVERED {
        let src = format!("{HEAD}{body}");
        for sabotage in [None, Some("noreachheldkeys"), Some("noreachprotoaccessors")] {
            assert!(
                hit_runs(&src, sabotage),
                "{what}: hit inactive ({sabotage:?})\n{src}"
            );
        }
    }
}

#[test]
fn a_function_never_stored_stays_inactive() {
    for (what, body) in NEVER {
        let src = format!("{HEAD}{body}");
        for sabotage in [None, Some("noreachheldkeys"), Some("noreachprotoaccessors")] {
            assert!(
                !hit_runs(&src, sabotage),
                "{what}: hit active ({sabotage:?})\n{src}"
            );
        }
    }
}

/// The `held` rows add own keys to shapes the table already names: the
/// shapes, and so every token's shape index, are the same with and without
/// them.
#[test]
fn the_held_rows_add_keys_and_no_shape() {
    use crate::builtins::realm_shapes::Declarations;
    let m = crate::model::Model::default();
    let (with, without) = (Declarations::build(true), Declarations::build(false));
    assert_eq!(with.names(), without.names());
    for (holder, key) in [
        ("Temporal.Duration", "prototype"),
        ("Temporal.Duration", "from"),
        ("Intl.NumberFormat", "prototype"),
        ("Intl.NumberFormat", "supportedLocalesOf"),
        ("WebAssembly.Module", "prototype"),
    ] {
        let own = |d: &Declarations| {
            let shape = d.named(holder, &m).expect(holder);
            d.property(shape, key, &m).is_some_and(|p| p.own_namespace)
        };
        assert!(own(&with), "{holder}.{key} is declared");
        assert!(!own(&without), "{holder}.{key} was not declared");
    }
    // A node module's function kept its rows in both.
    for d in [&with, &without] {
        let shape = d.named("node:stream.Readable", &m).expect("Readable");
        assert!(d
            .property(shape, "prototype", &m)
            .is_some_and(|p| p.own_namespace));
    }
}

//! NSPRIM: ToPrimitive (ECMA-262 7.1.1) of a builtin namespace object
//! (`Math`, `JSON`, `Reflect`, a realm host namespace such as `Atomics` or
//! `Intl`). No such object, nor `Object.prototype` (its [[Prototype]]),
//! holds `@@toPrimitive`: its [[Get]] reads `undefined` (10.1.8.1), so
//! OrdinaryToPrimitive (7.1.1.1) calls `Object.prototype.valueOf`
//! (20.1.3.7: the object itself, no primitive) and then
//! `Object.prototype.toString` (20.1.3.6: `"[object " + @@toStringTag +
//! "]"`, a string). The conversion runs no unknown code: `+Atomics` is a
//! number (NaN), `"" + Math` a string, a comparison a boolean. A value the
//! program stored there (`Math.toString = f`, `Atomics[Symbol.toPrimitive]
//! = g`, `Object.prototype.valueOf = h`) is what [[Get]] reads: that value
//! is the callee.
use super::*;
use crate::model::prim;

/// The kind of each `h.k<i> = reads[i]` after `prelude`; `h.flag` starts
/// as a string and a callee the program stored sets it to a number.
fn kinds(prelude: &str, reads: &[&str]) -> (Vec<crate::derive::KindFact>, crate::derive::KindFact) {
    let mut src = String::from("class H { flag: any = \"s\";");
    for i in 0..reads.len() {
        src += &format!(" k{i}: any = 0;");
    }
    src += " }\nconst h = new H();\n";
    src += prelude;
    for (i, r) in reads.iter().enumerate() {
        src += &format!("h.k{i} = {r};\nconsole.log(h.k{i});\n");
    }
    src += "console.log(h.flag);\n";
    let pf = facts(&src);
    let t = tmpl(&pf, "H");
    let ks = (0..reads.len())
        .map(|i| field(t, &format!("k{i}")).kind.clone())
        .collect();
    (ks, field(t, "flag").kind.clone())
}

const NAMESPACES: [&str; 5] = ["Atomics", "Intl", "Math", "JSON", "Reflect"];

#[test]
fn nsprim_namespace_conversions_are_primitives() {
    for ns in NAMESPACES {
        let o = format!("({ns} as any)");
        let reads = [
            format!("+{o}"),
            format!("\"\" + {o}"),
            format!("`${{{o}}}`"),
            format!("{o} < 1"),
            format!("String({o})"),
        ];
        let reads: Vec<&str> = reads.iter().map(|s| s.as_str()).collect();
        let (ks, flag) = kinds("", &reads);
        for (r, k) in reads.iter().zip(&ks) {
            assert!(!k.top, "{r}: {k:?}");
            assert!(
                k.objs.is_empty() && k.funcs == 0 && k.builtin_objs == 0,
                "{r}: {k:?}"
            );
        }
        assert_eq!(ks[0].prims, prim::NUM, "{}: {:?}", reads[0], ks[0]);
        assert!(!flag.top && flag.prims == prim::STR, "{ns}: flag {flag:?}");
    }
}

/// The program's stored methods are the callees: each sets `h.flag` to a
/// number. Controls for each kind of store: on the namespace under
/// `toString`, `valueOf`, `Symbol.toPrimitive`, and on `Object.prototype`.
#[test]
fn nsprim_program_stored_methods_are_the_callees() {
    for ns in NAMESPACES {
        for (store, conv) in [
            ("toString", "\"\" + o"),
            ("valueOf", "+o"),
            ("[Symbol.toPrimitive]", "+o"),
        ] {
            let dot = if store.starts_with('[') { "" } else { "." };
            let prelude = format!(
                "const o: any = {ns};\n\
                 o{dot}{store} = function (this: any): any {{ h.flag = 7; return 1; }};\n"
            );
            let (ks, flag) = kinds(&prelude, &[conv]);
            assert!(!ks[0].top, "{ns} {store}: {:?}", ks[0]);
            assert!(!flag.top, "{ns} {store}: flag {flag:?}");
            assert_eq!(
                flag.prims & prim::NUM,
                prim::NUM,
                "{ns} {store}: flag {flag:?}"
            );
        }
        let prelude = format!(
            "const o: any = {ns};\n\
             (Object.prototype as any).toString = function (this: any): any {{ h.flag = 7; return \"x\"; }};\n"
        );
        let (_, flag) = kinds(&prelude, &["\"\" + o"]);
        assert_eq!(
            flag.prims & prim::NUM,
            prim::NUM,
            "{ns} Object.prototype: flag {flag:?}"
        );
    }
}

/// The conversions make no ⊤ at all (the STOP_ON_TOP switch, captured): was
/// `result: argument of builtin Module("Atomics")[computed]` (builtins.rs
/// `unknown_method_proto_inner`), the `@@toPrimitive` lookup of a realm
/// host namespace taken as a computed-key call (c09, webgl.ts `+a1`).
#[test]
fn nsprim_namespace_conversions_make_no_top() {
    for ns in NAMESPACES {
        let src = format!(
            "const o: any = {ns};\n\
             const x = +o;\nconst s = \"\" + o;\nconst t = `${{o}}`;\nconst c = o < 1;\n\
             console.log(x, s, t, c);\n"
        );
        let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
        let mut cfg = FactsConfig::default();
        cfg.max_work.get_or_insert(50_000_000);
        crate::topstop::capture_on();
        let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
        let v = crate::topstop::take_captured();
        assert!(v.is_empty(), "{ns}: ⊤ made:\n{}", v.join("\n"));
    }
}

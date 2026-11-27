//! HOSTCHAIN: the `console` object is described by its realm declaration
//! (perry-runtime native_module.rs `js_create_native_module_namespace`, realm
//! entry `console`): its own keys, all methods, and its [[Prototype]],
//! `Object.prototype`. Neither holds `@@toPrimitive`, so ToPrimitive
//! (ECMA-262 7.1.1) reads `undefined` there (10.1.8.1) and
//! OrdinaryToPrimitive (7.1.1.1) calls `Object.prototype.valueOf` (the object
//! itself) and then `Object.prototype.toString` (a string). The conversion
//! runs no unknown code. A value the program stored there
//! (`console.toString = f`, `console[Symbol.toPrimitive] = g`) is what
//! [[Get]] reads: that value is the callee. Was: the chain undescribed
//! (`members::is_host`), `@@toPrimitive` taken as a computed key, ⊤ at
//! `result: argument of builtin Console[computed]` (c09, webgl.ts `+a1`).
use super::*;
use crate::model::prim;

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

const CONVERSIONS: [&str; 5] = ["+o", "\"\" + o", "`${o}`", "o < 1", "String(o)"];

#[test]
fn hostchain_console_conversions_are_primitives() {
    let (ks, flag) = kinds("const o: any = console;\n", &CONVERSIONS);
    for (r, k) in CONVERSIONS.iter().zip(&ks) {
        assert!(!k.top, "{r}: {k:?}");
        assert!(
            k.objs.is_empty() && k.funcs == 0 && k.builtin_objs == 0,
            "{r}: {k:?}"
        );
    }
    assert_eq!(ks[0].prims, prim::NUM, "+console: {:?}", ks[0]);
    assert!(!flag.top && flag.prims == prim::STR, "flag {flag:?}");
}

/// The program's stored methods are the callees: each sets `h.flag` to a
/// number (on console under `toString`, `valueOf`, `Symbol.toPrimitive`,
/// and on `Object.prototype`).
#[test]
fn hostchain_program_stored_methods_are_the_callees() {
    for (store, conv) in [
        ("toString", "\"\" + o"),
        ("valueOf", "+o"),
        ("[Symbol.toPrimitive]", "+o"),
    ] {
        let dot = if store.starts_with('[') { "" } else { "." };
        let prelude = format!(
            "const o: any = console;\n\
             o{dot}{store} = function (this: any): any {{ h.flag = 7; return 1; }};\n"
        );
        let (ks, flag) = kinds(&prelude, &[conv]);
        assert!(!ks[0].top, "{store}: {:?}", ks[0]);
        assert!(!flag.top, "{store}: flag {flag:?}");
        assert_eq!(flag.prims & prim::NUM, prim::NUM, "{store}: flag {flag:?}");
    }
    let prelude = "const o: any = console;\n\
         (Object.prototype as any).toString = function (this: any): any { h.flag = 7; return \"x\"; };\n";
    let (_, flag) = kinds(prelude, &["\"\" + o"]);
    assert_eq!(
        flag.prims & prim::NUM,
        prim::NUM,
        "Object.prototype: flag {flag:?}"
    );
}

/// The conversions make no ⊤ at all (the STOP_ON_TOP switch, captured).
#[test]
fn hostchain_console_conversions_make_no_top() {
    let src = "const o: any = console;\n\
         const x = +o;\nconst s = \"\" + o;\nconst t = `${o}`;\nconst c = o < 1;\n\
         console.log(x, s, t, c);\nconsole.log(\"%s\", String(o));\n";
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    assert!(v.is_empty(), "⊤ made:\n{}", v.join("\n"));
}

/// Control: a host object whose chain the realm does not declare keeps its
/// undescribed chain. `process.stdout` has two births chosen at run time
/// (`build_stream_object_with_write`, `tty::is_tty_fd`); the TTY one (its
/// tty class, chain `node:tty.WriteStream.prototype`) is undeclared, so
/// `@@toPrimitive` on it may be on that chain: the conversion still calls
/// an unknown method.
#[test]
fn hostchain_undeclared_stream_chain_keeps_unknown() {
    let src = "const o: any = process.stdout;\nconst x = +o;\nconsole.log(x);\n";
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    assert!(
        v.iter().any(|l| l.contains("[computed]")),
        "expected the undescribed-chain conversion, got:\n{}",
        v.join("\n")
    );
}

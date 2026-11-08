//! A store on a built-in constructor (`Object[k] = v`) is an own property
//! of that object (OrdinarySet). Program code reads it through the field
//! graph, and the operand protocols (ToPrimitive, `then`, the formatter's
//! hook) read it as fields of the token. The realm reads a constructor's
//! static by name only where `members::realm_read_static` says (V8's stack
//! hooks, the Promise combinators' `C.resolve`), and a static
//! of an undescribed constructor may be any. Only those stores sink (E20).
//! tsc's `setCachedIterationTypes` (`type[cacheKey] = cached`, `type` may be
//! `Object` by imprecision) sank every cached value.
use super::*;

fn facts(src: &str, interp: bool) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut pb);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    if interp {
        pb.dynamic_code_reach
            .record_global("new Function(...) @ x.ts (byte 1) (names globals)".into());
        cfg.dynamic_code = Some("new Function(...) @ x.ts (names globals)".into());
    }
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    crate::compute(&[(path, &hir)], &cfg)
}

/// Is the first `.v` read site after `/*A*/` proven?
fn v_proven(src: &str, interp: bool) -> (bool, String) {
    let pf = facts(src, interp);
    let at = src.find("/*A*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && !s.is_call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .v after /*A*/ in\n{}", pf.dump_text()));
    (!matches!(s.verdict, Verdict::No(_)), pf.dump_text())
}

fn program(store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const n = new N();
        const k: any = process.argv[2];
        {store}
        console.log(/*A*/ n.v);"
    )
}

/// The fixture: a computed-key store of `n` on `Object` (and on other
/// described constructors under a key the realm never reads) keeps `n` the
/// program's. The key `c` + any string may name a declared static
/// (`Object.create`) but never `__proto__`: a store under a key that may be
/// `__proto__` relinks the constructor's prototype to `n` and its members
/// escape (E6, `given_prototype`), a layer of its own.
#[test]
fn store_on_a_constructor_is_its_own_field() {
    let mut bad = Vec::new();
    for store in [
        "(Object as any)[`c${k}`] = n;",
        "(Object as any).keys = n;",
        "(Object as any).create = n;",
        "(Array as any)[\"c\" + k] = n;",
        "(String as any)[`c${k}`] = n;",
        "const h: any = process.argv.length > 3 ? Object : {}; h[`c${k}`] = n;",
    ] {
        let (p, dump) = v_proven(&program(store), false);
        if !p {
            bad.push(format!("{store}: n.v unproven (E20 sink)\n{}", &dump[..dump.len().min(4000)]));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A program read sees the stored value: `Object.keys` after `Object[k] = n`.
#[test]
fn a_read_of_a_declared_static_sees_the_store() {
    let src = "class N { v = 1; }
        const k: any = process.argv[2];
        (Object as any)[k] = new N();
        const back: any = (Object as any).keys;
        console.log(/*A*/ back.v);";
    let pf = facts(src, false);
    let at = src.find("/*A*/").unwrap() as u32;
    let s = pf.sites.iter().find(|s| s.key == "v" && s.byte_offset >= at).expect("back.v");
    assert!(
        !matches!(s.verdict, Verdict::No(ref w) if w.contains("no receiver")),
        "back.v reads the stored N\n{}",
        pf.dump_text()
    );
}

/// Controls: the stores the realm may read back keep the sink. A key that
/// may name V8's stack hooks or `Promise.resolve`, an undescribed
/// constructor's statics (Date's are described from the realm: see
/// `date_store_under_a_c_key_is_its_own_field`), a non-callable namespace, a prototype.
#[test]
fn realm_read_statics_still_sink() {
    let mut bad = Vec::new();
    for store in [
        "(Error as any)[k] = n;",
        "(Error as any)[`p${k}`] = n;",
        "(AggregateError as any)[`s${k}`] = n;",
        "(Promise as any)[`r${k}`] = n;",
        "(Error as any).prepareStackTrace = n;",
        "(Error as any).stackTraceLimit = n;",
        "(Promise as any)[k] = n;",
        "(Promise as any).resolve = n;",
        "(Date as any)[k] = n;",
        "(RegExp as any)[k] = n;",
        "(Math as any)[k] = n;",
        "(Object.prototype as any)[k] = n;",
    ] {
        let (p, _) = v_proven(&program(store), false);
        if p {
            bad.push(format!("{store}: n.v proven, the realm may read n"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Date is program-only and described from the realm: under node a key that
/// starts with "c" is neither `__proto__` nor any Date static (now, parse,
/// UTC, prototype), so the store adds an own data field no realm static reads.
#[test]
fn date_store_under_a_c_key_is_its_own_field() {
    let (p, dump) = v_proven(&program("(Date as any)[`c${k}`] = n;"), false);
    assert!(p, "n.v unproven (E20 sink)\n{}", &dump[..dump.len().min(4000)]);
}

/// Control for the test above: the same store, but `Date` is handed to code
/// the solver cannot see (an unknown function): the callee may read any
/// field of it, so `n.v` stays unproven.
#[test]
fn date_store_under_a_c_key_sinks_once_date_escapes() {
    let mut bad = Vec::new();
    for (store, interp) in [
        ("(Date as any)[`c${k}`] = n; (globalThis as any).u(Date);", false),
        ("(globalThis as any).u(Date); (Date as any)[`c${k}`] = n;", false),
    ] {
        let (p, _) = v_proven(&program(store), interp);
        if p {
            bad.push(format!("{store} (interp {interp}): n.v proven, Date escaped"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Control: with the interpreter linked, interpreted code reads any field of
/// `Object` (rule R1: the built-in function escapes).
#[test]
fn interpreted_code_still_reads_it() {
    let (p, _) = v_proven(&program("(Object as any)[k] = n;"), true);
    assert!(!p, "interpreted code may read Object[k]: n escapes");
}

/// Controls: what the program stored on the constructor is what later reads
/// of it run: an operand protocol (ToPrimitive's `toString`, the `then` of
/// an awaited value, `@@iterator`), a program call of a declared static
/// (`Object.keys`), a member found through a stored `__proto__`. The stored
/// function's `delete n.v` runs.
#[test]
fn protocols_call_the_stored_function() {
    let mut bad = Vec::new();
    for (store, use_) in [
        ("(Object as any)[k] = function () { delete (n as any).v; return \"o\"; };", "String(Object);"),
        ("(Object as any).toString = function () { delete (n as any).v; return \"o\"; };", "console.log(`${Object}`);"),
        (
            "(Object as any)[k] = function (r: any) { delete (n as any).v; r(1); };",
            "(async () => { await (Object as any); })();",
        ),
        ("(Object as any)[k] = function (a: any) { delete a.v; return []; };", "Object.keys(n);"),
        ("(Object as any).keys = function (a: any) { delete a.v; return []; };", "Object.keys(n);"),
        (
            "(Object as any)[k] = { foo() { delete (n as any).v; } };",
            "(Object as any).foo();",
        ),
        (
            "(Object as any)[k] = function* () { delete (n as any).v; };",
            "for (const x of (Object as any)) console.log(x);",
        ),
    ] {
        let (p, _) = v_proven(&program(&format!("{store}\n{use_}")), false);
        if p {
            bad.push(format!("{store} {use_}: n.v proven, the protocol call was missed"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Controls: SpeciesConstructor and ArraySpeciesCreate read `O.constructor`
/// and then its `@@species`. With `O.constructor` set to `Object`, the
/// function the program stored on `Object` under `@@species` (or a key that
/// may name it) is the
/// species they construct (node: `delete n.v` runs for each row but the
/// first, where `Array`'s own getter-only `@@species` ignores the store).
#[test]
fn species_reads_see_the_store() {
    let f = "(Object as any)[Symbol.species] = function (ex: any) { delete (n as any).v; if (typeof ex === \"function\") ex(() => {}, () => {}); return [] as any; };";
    let mut bad = Vec::new();
    for (store, use_) in [
        (
            "(Array as any)[k] = function () { delete (n as any).v; return []; };".to_string(),
            "[1].map((x) => x);",
        ),
        (f.replace("[Symbol.species]", "[k]"), "const a: any = [1]; a.constructor = Object; a.map((x: any) => x);"),
        (f.replace("[Symbol.species]", "[k]"), "const p: any = Promise.resolve(1); p.constructor = Object; p.then((x: any) => x);"),
        (f.to_string(), "const a: any = [1]; a.constructor = Object; a.map((x: any) => x);"),
        (f.to_string(), "const p: any = Promise.resolve(1); p.constructor = Object; p.then((x: any) => x);"),
        (f.to_string(), "const r: any = /a/; r.constructor = Object; \"abc\".split(r);"),
        (f.to_string(), "const u: any = new Uint8Array(2); u.constructor = Object; u.slice(0);"),
        (f.to_string(), "const p: any = Promise.resolve(1); p.constructor = Object; p.catch((x: any) => x);"),
        (f.to_string(), "const p: any = Promise.resolve(1); p.constructor = Object; p.finally(() => {});"),
        (f.to_string(), "const r: any = /a/g; r.constructor = Object; [...\"abc\".matchAll(r)];"),
        (f.to_string(), "const b: any = new ArrayBuffer(8); b.constructor = Object; b.slice(0);"),
    ] {
        let (p, _) = v_proven(&program(&format!("{store}\n{use_}")), false);
        if p {
            bad.push(format!("{store} {use_}: n.v proven, the species construct was missed"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

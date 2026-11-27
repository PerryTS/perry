//! Rule KEYSET: a computed store whose key holds only a finite set of known
//! String constants writes each of those named keys (ECMA-262 7.1.19
//! ToPropertyKey, 10.1.9 OrdinarySet: the store writes exactly the key it
//! is given), never an any-key store. The app's loops (pkg/web
//! audio/index.ts:27, browser.ts:323) `for (const [k, v] of
//! Object.entries(classes)) g[k] = v` over a literal installed on the
//! global object then write the literal's names only: the global binding
//! `Float32Array` (13.3.5.1 evaluates it for `new`) stays the realm's
//! intrinsic, none of the stored classes.
//! Only a key with an unknown part keeps the any-key store (GLOBK G2).
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The receivers of the first `.key` site after `marker` (and whether it
/// may be ⊤), with the dump.
fn receivers(src: &str, key: &str, marker: &str) -> (Vec<String>, bool, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    eprintln!("KEYSET {key}@{marker}: {:?} top={} {:?}", s.receivers, s.top, s.verdict);
    (s.receivers.clone(), s.top, pf.dump_text())
}

/// The classes the loop installs, then a read of the global binding
/// `Float32Array`, a `new` of it and a read on what it made (a literal
/// `new Float32Array(n)` is HIR's typed-array allocation, which reads no
/// binding); `classes_decl` declares `classes`, `loop_stmt` is the loop.
fn program(classes_decl: &str, loop_stmt: &str) -> String {
    format!(
        "class Alpha {{ alpha = 1; length = 7; }}
        class Beta {{ beta = 2; length = 8; }}
        function mkGamma(f: () => unknown): unknown {{ return class Gamma {{ gamma = f(); length = 9; }}; }}
        const g = globalThis as Record<string, unknown>;
        {classes_decl}
        {loop_stmt}
        const F: any = Float32Array;
        const f: any = new F(4);
        console.log(/*F*/ f.length);"
    )
}

fn stored_class(recv: &[String]) -> bool {
    recv.iter()
        .any(|r| r.contains("Alpha") || r.contains("Beta") || r.contains("Gamma"))
}

const LOOP: &str = "for (const [k, v] of Object.entries(classes)) g[k] = v;";

/// RED on base: the app's audio loop. The literal typed as a record, with a
/// class made by a call among its values (audio/index.ts `Audio:
/// makeAudioClass(...)`): its keys are the literal's names, so the store
/// names `Alpha`, `Beta`, `Audio` only and `new Float32Array` constructs
/// the intrinsic alone.
#[test]
fn keyset_entries_record_literal() {
    for decl in [
        "const classes = { Alpha, Beta };",
        "const classes: Record<string, unknown> = { Alpha, Beta };",
        "const classes: Record<string, unknown> = { Alpha, webkitAlpha: Alpha, Beta, Audio: mkGamma(() => 1) };",
    ] {
        let (r, top, dump) = receivers(&program(decl, LOOP), "length", "/*F*/");
        assert!(
            !top && !r.is_empty() && !stored_class(&r),
            "{decl}: f is a Float32Array only, got {r:?} top={top}\n{dump}"
        );
    }
}

/// RED on base: the browser.ts loop, whose store is guarded by tests of
/// the key (`k === "..."`, `typeof g[k]`, `k.startsWith("HTML")`).
#[test]
fn keyset_entries_guarded_loop() {
    let decl = "const classes: Record<string, unknown> = { Alpha, HTMLBeta: Beta, Window: mkGamma(() => 1) };";
    let lp = "for (const [k, v] of Object.entries(classes)) if (k === \"Alpha\" || typeof g[k] === \"undefined\" || k.startsWith(\"HTML\")) g[k] = v;";
    let (r, top, dump) = receivers(&program(decl, lp), "length", "/*F*/");
    assert!(
        !top && !r.is_empty() && !stored_class(&r),
        "f is a Float32Array only, got {r:?} top={top}\n{dump}"
    );
}

/// RED on base: a key that is one of two literals (no entries loop).
#[test]
fn keyset_two_literal_key() {
    let lp = "const k = process.argv.length > 2 ? \"Alpha\" : \"Beta\"; g[k] = process.argv.length > 3 ? Alpha : Beta;";
    let (r, top, dump) = receivers(&program("", lp), "length", "/*F*/");
    assert!(
        !top && !r.is_empty() && !stored_class(&r),
        "f is a Float32Array only, got {r:?} top={top}\n{dump}"
    );
}

/// The stored names stay readable: `new (g.Alpha)` / a global read `Beta`
/// by name still sees the stored class (the store is named, not dropped).
#[test]
fn keyset_named_reads_see_store() {
    let decl = "const classes: Record<string, unknown> = { Alpha2: Alpha, Beta };";
    let src = format!(
        "{}\nconst a: any = new (g as any).Alpha2();\nconsole.log(/*A*/ a.alpha);",
        program(decl, LOOP)
    );
    let (r, _top, dump) = receivers(&src, "alpha", "/*A*/");
    assert!(
        r.iter().any(|x| x.contains("Alpha")),
        "g.Alpha2 is the stored Alpha, got {r:?}\n{dump}"
    );
}

/// KEEP (control): a key with an unknown part (`process.argv`) stays an
/// any-key store, which every global binding read it can spell takes:
/// `new Float32Array` may construct the stored class.
#[test]
fn keyset_keep_unknown_key() {
    for lp in [
        "g[String(process.argv[2])] = Alpha;",
        "g[\"Float\" + String(process.argv[2])] = Alpha;",
    ] {
        let (r, top, dump) = receivers(&program("", lp), "length", "/*F*/");
        assert!(
            top || stored_class(&r),
            "{lp}: the any-key store reaches Float32Array, got {r:?} top={top}\n{dump}"
        );
    }
}

/// RED on base (pkg/web audio/index.ts:27 in the app, where three.js
/// stores under a computed key into an Array literal): a computed-key store
/// into another Array sets the runtime's member-writable flag, so
/// `array_record_enter` asks each Array for its `@@iterator`; the entry
/// pair, born with `%Array.prototype%` and never written, still answers the
/// intrinsic one, so `[k, v]` reads its index slots (rule RP2) and `k`
/// holds the keys alone, never the stored classes.
#[test]
fn keyset_entries_beside_keyed_array_store() {
    let decl = "const other: any[] = []; other[String(process.argv[2])] = 1;
        const classes: Record<string, unknown> = { Alpha, Beta, Audio: mkGamma(() => 1) };";
    let (r, top, dump) = receivers(&program(decl, LOOP), "length", "/*F*/");
    assert!(
        !top && !r.is_empty() && !stored_class(&r),
        "f is a Float32Array only, got {r:?} top={top}\n{dump}"
    );
}

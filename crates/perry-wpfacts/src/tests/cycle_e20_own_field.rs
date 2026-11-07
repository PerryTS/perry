//! A number-key store onto a built-in function or namespace object writes
//! that object's own numeric field: the realm declares no index static and
//! the runtime reads no index of such an object, so it is no E20 sink
//! (tsc's `indexInfos[i] = ...`, where the holder may be `Object` by
//! imprecision, sank every stored value). The global object's indices,
//! prototypes and keys that may name a declared static keep the sink.
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

/// Is the one site of `.v` after `marker` proven?
fn v_proven(src: &str, marker: &str) -> (bool, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .v after {marker:?} in\n{}", pf.dump_text()));
    (!matches!(s.verdict, Verdict::No(_)), pf.dump_text())
}

/// `holder` may be one of the values in `holders`; `store` stores `n`.
fn program(holders: &str, store: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const hs: any[] = [{holders}];
        const h: any = hs[Math.random() > 0.5 ? 1 : 0];
        const i = Math.random() > 0.5 ? 1 : 0;
        const k: any = process.argv[2];
        const n = new N();
        {store}
        console.log(/*A*/ n.v, h[i]);"
    )
}

#[test]
fn number_key_store_on_builtin_is_own_field() {
    for holders in ["Object, Math", "Object, Array", "Math, JSON", "Object.keys, Math"] {
        let (p, dump) = v_proven(&program(holders, "h[i] = n;"), "/*A*/");
        assert!(p, "[{holders}]: an own index, no sink: n.v proven\n{dump}");
    }
}

/// The store is read back: what `h[i]` reads holds `n` (the own field).
#[test]
fn read_sees_the_stored_value() {
    let src = "class N { v = 1; }
        const hs: any[] = [Object, Math];
        const h: any = hs[Math.random() > 0.5 ? 1 : 0];
        const i = Math.random() > 0.5 ? 1 : 0;
        h[i] = new N();
        const back = h[i];
        back.v = 2;
        const m = new N();
        console.log(/*A*/ m.v, back.v);";
    let (p, dump) = v_proven(src, "/*A*/");
    assert!(p, "m is no stored value\n{dump}");
    let pf = facts_of(src);
    let at = src.find("back.v)").unwrap() as u32;
    let s = pf.sites.iter().find(|s| s.key == "v" && s.byte_offset >= at).expect("back.v");
    assert!(
        !matches!(s.verdict, Verdict::No(ref w) if w.contains("no receiver")),
        "back.v reads the stored N\n{dump}"
    );
}

/// Controls: the unsound directions stay closed. A key that may name a
/// declared static, the global object's index, a prototype's index.
#[test]
fn other_stores_still_sink() {
    for (holders, store) in [
        ("Object, Math", "h[k] = n;"),
        ("Object, Math", "(globalThis as any)[i] = n;"),
        ("Object, Math", "(String.prototype as any)[i] = n;"),
    ] {
        let (p, dump) = v_proven(&program(holders, store), "/*A*/");
        assert!(!p, "{store}: still an E20 sink, n escapes\n{dump}");
    }
}

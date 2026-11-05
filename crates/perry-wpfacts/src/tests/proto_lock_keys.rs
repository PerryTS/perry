//! DEC 250 (Q2), rule L: the runtime refuses an add on `Object.prototype`
//! and `Function.prototype` and an index add on `Array.prototype`
//! (`builtin_proto::add_locked`, `refuse_index_add`), so a store into ⊤
//! under a number key writes no key a plain object's or an array's chain
//! reads (`members::locked_read` of `NUM_KEY`). A function table called as
//! `table[k](n)` (tsc's `forEachChildTable[node.kind](...)`) then calls
//! only its own fields: `n` stays the program's. The chains the runtime
//! leaves open (a Map's prototype; `constructor`, which the lock exempts)
//! still take the write.
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

/// `holder` is the receiver of `holder[k](n)`; `store` runs before.
fn program(holder: &str, store: &str, key: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        const tab: any = {holder};
        const top: any = (globalThis as any).someHost;
        {store}
        function run(k: any, n: N): number {{ return tab[k](n); }}
        console.log(run({key}, new N()), run({key}, new N()));"
    )
}

const TABLE: &str = "{ 0: function (n: N) { return /*A*/ n.v; }, 1: (n: N) => n.v + 1 }";
const ARRAY: &str = "[function (n: N) { return /*A*/ n.v; }, (n: N) => n.v + 1]";

/// The fixture: a store into ⊤ under a number key, then a number-key call
/// of a table: the table's own fields are its callees, `n.v` is proven
/// (the lock stack head replayed the store: `n` escaped).
#[test]
fn number_key_top_store_writes_no_locked_chain() {
    for holder in [TABLE, ARRAY] {
        let src = program(holder, "top[0] = 1;", "Math.random() > 0.5 ? 1 : 0");
        let (p, dump) = v_proven(&src, "/*A*/");
        assert!(p, "{holder}: n.v must be proven\n{dump}");
    }
}

/// A computed key may name `constructor`, which the lock exempts: a store
/// into ⊤ under a computed key may have written it, so a computed call
/// runs what was stored and `n` escapes; a number-key store did not.
#[test]
fn computed_key_meets_the_exempt_constructor() {
    let key = "String(Math.random())";
    let (p, dump) = v_proven(&program(TABLE, "top[String(Math.random())] = 1;", key), "/*A*/");
    assert!(!p, "a computed ⊤ store may write `constructor`\n{dump}");
    let (p, dump) = v_proven(&program(TABLE, "top[0] = 1;", key), "/*A*/");
    assert!(p, "a number-key ⊤ store writes no `constructor`\n{dump}");
}

/// Soundness arm: a chain the runtime leaves open (a Map's prototype
/// takes adds) still reads what a number-key store into ⊤ wrote: the
/// call may run it with `n`, which then escapes (its `v` may be rewritten).
#[test]
fn open_chain_still_takes_the_number_key_write() {
    let src = |store: &str| {
        format!(
            "class N {{ v = 1; }}
            const top: any = (globalThis as any).someHost;
            {store}
            const mp: any = new Map();
            function run(k: number, n: N): number {{ try {{ mp[k](n); }} catch {{}} return /*A*/ n.v; }}
            console.log(run(Math.random() > 0.5 ? 1 : 0, new N()));"
        )
    };
    let (p, dump) = v_proven(&src("top[0] = 1;"), "/*A*/");
    assert!(!p, "the Map chain must replay the number-key write\n{dump}");
    // The arm can pass: with no store into ⊤ the key is absent.
    let (p, dump) = v_proven(&src(""), "/*A*/");
    assert!(p, "no write: mp[k] is absent, n stays the program's\n{dump}");
}

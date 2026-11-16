//! SKINROOT11: every store whose key may be `"__proto__"` gives its value
//! to a JSON object's prototype (B.2.2.1.2), not only the first one.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn kept(pf: &ProgramFacts) -> crate::derive::KindFact {
    let t = pf
        .templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == "keptv"))
        .unwrap_or_else(|| panic!("no keptv\n{}", pf.dump_text()));
    t.fields.iter().find(|f| f.key == "keptv").unwrap().kind.clone()
}

/// SKINROOT11 step 1: two `__proto__`-capable stores into one JSON object
/// (unknown keys `k1`, `k2`): each may be `"__proto__"` (B.2.2.1.2), so
/// each stored object may become j's prototype; a read of `j.tag` through
/// the chain may find either `tag`.
#[test]
fn skinroot11_second_proto_store_reaches_prototype() {
    let src = "const j: any = JSON.parse(process.argv[2]);
         const k1: any = process.argv[3];
         const k2: any = process.argv[4];
         const a: any = { tag: function fa() {} };
         const b: any = { tag: function fb() {} };
         j[k1] = a;
         j[k2] = b;
         const r: any = { keptv: 0 };
         r.keptv = j.tag;
         console.log(String(r.keptv));";
    let pf = whole(src);
    let k = kept(&pf);
    assert!(k.top || k.funcs >= 2, "{k:?}\n{}", pf.dump_text());
}

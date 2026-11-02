//! CHB2: a class private field read is brand-guarded (`PrivateGuard`): a
//! receiver without the element throws first, so the read never sees the key
//! absent (no `undefined` for absence), and it consults no prototype. The
//! guard's value is its receiver's. Before, the read of a private Map field
//! held `undefined` beside the Map: calls on it were "primitive receiver"
//! or never NativeMember, and the entries a `get` returns did not flow.
use super::*;

fn modules(mods: &[(&str, &str)], per_unit: bool) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

fn verdicts(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn all(v: &[Verdict], want: fn(&Verdict) -> bool) -> bool {
    !v.is_empty() && v.iter().all(want)
}

const TABLE: &str = r#"
export class E {
  #t: Map<string, any> = new Map<string, any>();
  n: number = 0;
  put(k: string, v: number): void {
    if (this.#t.size > 9) this.#t.clear();
    this.#t.set(k, { v, d: 1 });
  }
  at(k: string): number {
    const e = this.#t.get(k);
    this.n = this.n + 1;
    return e === undefined ? 0 : e.v + e.d;
  }
}
"#;
const MAIN: &str =
    "import { E } from './a'; const e = new E(); e.put('a', 1); console.log(e.at('a'), e.n);";

#[test]
fn a_private_map_field_initializer_reaches_its_reads() {
    let pf = modules(&[("a", TABLE), ("m", MAIN)], false);
    let dump = || pf.dump_text();
    // Base: "primitive receiver" (the read held only `undefined`).
    for key in ["set", "get", "clear"] {
        let v = verdicts(&pf, key, true);
        assert!(all(&v, |v| *v == Verdict::NativeMember), "{key}: {v:?}\n{}", dump());
    }
    let v = verdicts(&pf, "size", false);
    assert!(all(&v, |v| *v == Verdict::NativeMember), "size: {v:?}");
    // What `get` returns is the literal `set` stored (base: dead site).
    for key in ["v", "d"] {
        let v = verdicts(&pf, key, false);
        assert!(all(&v, |v| matches!(v, Verdict::T0Mono)), "{key}: {v:?}\n{}", dump());
    }
    // Per unit is never stronger than whole, and in both modes the private
    // key is no public layout key: the public field keeps its slot and the
    // class its fixed layout.
    let pu = modules(&[("a", TABLE), ("m", MAIN)], true);
    for s in &pu.sites {
        if matches!(s.verdict, Verdict::No(_)) {
            continue;
        }
        let w = pf
            .sites
            .iter()
            .find(|x| x.module == s.module && x.byte_offset == s.byte_offset && x.key == s.key);
        assert!(
            w.is_some_and(|w| !matches!(w.verdict, Verdict::No(_))),
            "per unit proves {} at {} but whole does not",
            s.key,
            s.byte_offset
        );
    }
    for p in [&pf, &pu] {
        let n = verdicts(p, "n", false);
        assert!(all(&n, |v| matches!(v, Verdict::T0Mono)), "n: {n:?}");
        let e = p
            .templates
            .iter()
            .find(|t| t.display.ends_with(":E"))
            .expect("E");
        assert!(e.prefix_stable && e.fixed_layout, "{}", p.dump_text());
        let n = e.fields.iter().find(|f| f.key == "n").expect("n");
        assert_eq!(n.slot, Some(0), "{}", p.dump_text());
    }
}

#[test]
fn a_private_field_value_reaches_a_public_copy() {
    // Base: `pub` held only `num|undef`, while it holds the object.
    let pf = facts(
        r#"
        class C { #o: any = { a: 1 }; pub: any = 0; m(): void { this.pub = this.#o; } }
        const c = new C(); c.m(); console.log(c.pub.a);
        "#,
    );
    let p = field(tmpl(&pf, "C"), "pub");
    assert!(p.kind.desc().contains("obj{"), "{}\n{}", p.kind.desc(), pf.dump_text());
    let a = verdicts(&pf, "a", false);
    assert!(all(&a, |v| !matches!(v, Verdict::No(r) if r.starts_with("primitive"))), "{a:?}");
}

#[test]
fn a_private_map_handed_to_unknown_code_keeps_its_calls() {
    let src = r#"
        declare const ext: any;
        class E {
          #t: Map<string, any> = new Map<string, any>();
          leak(): void { ext(this.#t); }
          at(k: string): any { return this.#t.get(k); }
        }
        const e = new E(); e.leak(); console.log(e.at("a"));
    "#;
    let pf = facts(src);
    let v = verdicts(&pf, "get", true);
    assert!(all(&v, |v| matches!(v, Verdict::No(_))), "{v:?}\n{}", pf.dump_text());
    // Control: without the leak the same call is the Map's own member.
    let pf = facts(&src.replace("ext(this.#t);", ""));
    let v = verdicts(&pf, "get", true);
    assert!(all(&v, |v| *v == Verdict::NativeMember), "{v:?}\n{}", pf.dump_text());
}

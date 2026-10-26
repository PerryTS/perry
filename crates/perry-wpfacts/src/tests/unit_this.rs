//! Per-unit mode: a published method's receiver is a contract parameter
//! (design §17.11, DECISIONS 57).

use super::*;

fn units_of(mods: &[(&str, &str)]) -> ProgramFacts {
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
    crate::compute_units(
        &list,
        &FactsConfig {
            explain: true,
            ..Default::default()
        },
    )
}

#[test]
#[ignore]
fn scratch_this() {
    let cases = [
        ("export class R { k: number; constructor() { this.k = 1; } get() { return this.k + 1; } }",
         "import { R } from './a'; class S extends R { j: number; constructor() { super(); this.j = 2; } sum() { return this.get() + this.j; } } function f() { return new S().sum(); } console.log(f());"),
        ("export let g: any = null; export class R { k: number; constructor() { this.k = 1; g = this; } get() { return this.k + 1; } }",
         "import { R } from './a'; class S extends R { j: number; constructor() { super(); this.j = 2; } sum() { return this.get() + this.j; } } function f() { return new S().sum(); } console.log(f());"),
        ("export class R { k: number; constructor() { this.k = 1; } get() { return this.m() + 1; } m() { return 1; } }",
         "import { R } from './a'; class S extends R { j: number; constructor() { super(); this.j = 2; } m() { return this.j; } sum() { return this.get() + this.j; } } function f() { return new S().sum(); } console.log(f());"),
        ("export let g: any = null; export class R { k: number; constructor() { this.k = 1; } keep() { g = (this as any).x; return this.k; } }",
         "import { R } from './a'; class P { v: number; constructor() { this.v = 3; } } class S extends R { x: P; constructor() { super(); this.x = new P(); } sum() { this.keep(); return this.x.v; } } function f() { return new S().sum(); } console.log(f());"),
        ("export class R { append(p: any, q: any) { p.link = q; return p; } }",
         "import { R } from './a'; class P { link: any; v: number; constructor() { this.link = null; this.v = 3; } } class S extends R { constructor() { super(); } run() { const p = new P(); this.append(p, null); return p.v; } } function f() { return new S().run(); } console.log(f());"),
        ("export function append(p: any, q: any) { p.link = q; return p; }",
         "import { append } from './a'; class P { link: any; v: number; constructor() { this.link = null; this.v = 3; } } class S { run() { const p = new P(); append(p, null); return p.v; } } function f() { return new S().run(); } console.log(f());"),
    ];
    let only: Option<usize> = std::env::var("CW2G_CASE").ok().and_then(|v| v.parse().ok());
    for (i, (a, m)) in cases.into_iter().enumerate() {
        if only.is_some_and(|o| o != i + 1) {
            continue;
        }
        let pf = units_of(&[("a", a), ("m", m)]);
        eprintln!(
            "=====\n{}",
            pf.dump_text()
                .lines()
                .filter(|l| l.starts_with("S ")
                    || l.starts_with("# rounds")
                    || l.starts_with("# uout")
                    || l.starts_with("# broken"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        eprintln!("broken {:?}", pf.broken);
    }
}

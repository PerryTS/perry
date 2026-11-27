// HOSTSURF H2 (HSB2, 2026-11-27): the solver reads a provider's host
// surface. A TEST provider manifest (`hostsurf/tdom.json`, the feather DOM
// shape, no feather) is loaded beside the static realm. The 22-site
// workload (`hostsurf/native_click.ts`, a copy of feather's
// workload/native-click.ts) proves every host site; the soundness fixtures
// F1-F5 (HOSTSURF.md §5) keep the sites the program can change off the
// host verdicts.
use super::*;
use perry_hir::Module;

const TDOM: &str = include_str!("hostsurf/tdom.json");
const CLICK: &str = include_str!("hostsurf/native_click.ts");

fn host() -> &'static crate::host::HostRealm {
    let s = perry_hir::realm_manifest::host_surface::parse(TDOM, "tdom.json").expect("parses");
    crate::host::load(&[s]).expect("loads").expect("one surface")
}

fn facts(src: &str, with_host: bool) -> crate::ProgramFacts {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    let h = vec![(path, hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        host: with_host.then(host),
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// `(key, store, verdict)` of every site of a.ts, in source order.
fn sites(pf: &crate::ProgramFacts) -> Vec<(u32, String, bool, String)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let mut v: Vec<(u32, String, bool, String)> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi)
        .map(|s| {
            let v = match &s.verdict {
                crate::Verdict::HostMember { member, nullable, .. } => format!("host:member={member}{}", if *nullable { ",nullable" } else { "" }),
                crate::Verdict::HostGet { member, nullable, .. } => format!("host:get={member}{}", if *nullable { ",nullable" } else { "" }),
                crate::Verdict::HostSet { member, nullable, .. } => format!("host:set={member}{}", if *nullable { ",nullable" } else { "" }),
                other => format!("{other:?}"),
            };
            (s.byte_offset, s.key.clone(), s.access.is_store(), format!("{v} top={} recv={:?}", s.top, s.receivers))
        })
        .collect();
    v.sort();
    for (o, k, st, v) in &v {
        eprintln!("HSB2 S @{o} .{k} store={st} {v}");
    }
    v
}

fn of<'a>(v: &'a [(u32, String, bool, String)], key: &str, store: bool) -> Vec<&'a str> {
    v.iter().filter(|s| s.1 == key && s.2 == store).map(|s| s.3.as_str()).collect()
}

const HOST_KEYS: &[&str] = &[
    "document", "getElementById", "addEventListener", "createElement", "appendChild", "removeChild",
];

#[test]
fn hsb2_workload_proves_every_host_site() {
    let v = sites(&facts(CLICK, true));
    let mut n = 0;
    for k in HOST_KEYS {
        for s in of(&v, k, false) {
            assert!(s.starts_with("host:member="), ".{k}: {s}");
            n += 1;
        }
    }
    assert_eq!(n, 22, "the workload's 22 host sites");
    let text = of(&v, "textContent", true);
    assert!(!text.is_empty());
    for s in text {
        assert!(s.starts_with("host:set=tdom:Node.prototype.textContent.<set>"), ".textContent=: {s}");
    }
}

#[test]
fn hsb2_without_the_surface_no_host_verdict() {
    let v = sites(&facts(CLICK, false));
    assert!(v.iter().all(|s| !s.3.starts_with("host:")));
}

// F1: the program patches HTMLElement.prototype.appendChild, through the
// interface object and through getPrototypeOf: no appendChild site of an
// element is a host member; the run reaches the program's function.
#[test]
fn hsb2_f1_program_patch_of_a_host_prototype() {
    for patch in [
        "(globalThis as any).HTMLElement.prototype.appendChild = f;",
        "Object.getPrototypeOf(Object.getPrototypeOf(document.createElement('p'))).appendChild = f;",
    ] {
        let src = format!(
            "const document: any = (globalThis as any).document;\n\
             const records: any = document.getElementById('records');\n\
             const log: any[] = [];\n\
             function f(this: any, n: any): any {{ log.push(n); return n; }}\n\
             {patch}\n\
             const row: any = document.createElement('article');\n\
             const cell: any = document.createElement('p');\n\
             row.appendChild(cell);\n\
             records.appendChild(row);\n\
             console.log(log.length);\n"
        );
        let v = sites(&facts(&src, true));
        let app = of(&v, "appendChild", false);
        assert_eq!(app.len(), 2, "{patch}");
        for s in app {
            assert!(!s.starts_with("host:"), "{patch}: {s}");
        }
        // The program's function runs: its `log.push` site is live.
        assert!(of(&v, "push", false).iter().all(|s| !s.starts_with("Dead")), "{patch}");
    }
}

// F2: a store under an unknown key onto a host prototype drops every host
// verdict over that chain.
#[test]
fn hsb2_f2_unknown_key_store_onto_a_host_prototype() {
    let src = "const document: any = (globalThis as any).document;\n\
               const records: any = document.getElementById('records');\n\
               const k = String(Math.random());\n\
               (globalThis as any).HTMLElement.prototype[k] = function (n: any): any { return n; };\n\
               records.appendChild(document.createElement('p'));\n";
    let v = sites(&facts(src, true));
    let app = of(&v, "appendChild", false);
    assert_eq!(app.len(), 1);
    assert!(!app[0].starts_with("host:"), "{}", app[0]);
}

// F3: an own expando on one element: its sites take the program function;
// the other elements' sites stay host members. (`row` stays out of the
// `nodes` pool: an element appended there joins every pool read, so
// `records` could be `row` as far as the pool model knows.)
#[test]
fn hsb2_f3_per_instance_expando() {
    let src = "const document: any = (globalThis as any).document;\n\
               const records: any = document.getElementById('records');\n\
               const row: any = document.createElement('article');\n\
               row.appendChild = function (n: any): any { return n; };\n\
               row.appendChild(document.createElement('p'));\n\
               records.appendChild(document.createElement('p'));\n";
    let v = sites(&facts(src, true));
    let app = of(&v, "appendChild", false);
    assert_eq!(app.len(), 2);
    assert!(!app[0].starts_with("host:"), "row: {}", app[0]);
    assert!(app[1].starts_with("host:member=tdom:Node.prototype.appendChild"), "records: {}", app[1]);
}

// F4: setPrototypeOf relinks an element: no host verdict on it.
#[test]
fn hsb2_f4_set_prototype_of() {
    let src = "const document: any = (globalThis as any).document;\n\
               const row: any = document.createElement('article');\n\
               Object.setPrototypeOf(row, { appendChild(n: any): any { return n; } });\n\
               row.appendChild(document.createElement('p'));\n";
    let v = sites(&facts(src, true));
    let app = of(&v, "appendChild", false);
    assert_eq!(app.len(), 1);
    assert!(!app[0].starts_with("host:"), "{}", app[0]);
}

// F5: a listener stores a program object on a kept element; read back
// through getElementById (the pool), the value flows (no lost value).
#[test]
fn hsb2_f5_listener_value_read_back_through_the_pool() {
    let src = "const document: any = (globalThis as any).document;\n\
               const box: any = document.getElementById('box');\n\
               const cell: any = document.createElement('p');\n\
               box.appendChild(cell);\n\
               box.addEventListener('click', function (): void { cell.payload = { v: 1 }; });\n\
               const again: any = document.getElementById('cell');\n\
               if (again !== null && again.payload !== undefined) console.log(again.payload.v);\n";
    let v = sites(&facts(src, true));
    let rd = of(&v, "v", false);
    assert_eq!(rd.len(), 1);
    assert!(!rd[0].starts_with("Dead") && !rd[0].starts_with("Throws"), "{}", rd[0]);
    assert!(rd[0].contains("recv=[") && !rd[0].contains("recv=[]"), "{}", rd[0]);
}

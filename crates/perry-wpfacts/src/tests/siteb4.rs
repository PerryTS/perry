// SITEB4 (2026-11-28): fixtures for c09 "key not a definite birth field"
// (three.js OrbitControls fields read in bound listener functions and methods).
// Proven = a verdict that is neither No(..) nor Throws.
use super::*;
use perry_hir::Module;

fn facts_at(path: &str, src: &str) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(src, path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", path).expect("lowers");
    let h = vec![(path.to_string(), hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn reads(pf: &crate::ProgramFacts, path: &str, key: &str) -> Vec<(u32, bool, crate::Verdict)> {
    let mi = pf.module_paths.iter().position(|p| p == path).expect("module") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| (s.byte_offset, s.is_call, s.verdict.clone()))
        .collect()
}

fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws)
}

fn show_class(tag: &str, pf: &crate::ProgramFacts, class: &str) {
    for t in pf.templates.iter().filter(|t| t.display.ends_with(class)) {
        eprintln!("SB4 {tag} tmpl {} fixed_layout={} birth_keys={:?}", t.display, t.fixed_layout, t.birth_keys);
        for f in &t.fields {
            eprintln!(
                "SB4 {tag}   field {} slot={:?} definite={} data={} wc={} wp={}",
                f.key, f.slot, f.definite, f.data_field, f.writers_ctor, f.writers_post
            );
        }
    }
}

fn facts_multi(mods: &[(&str, &str)]) -> crate::ProgramFacts {
    let h: Vec<(String, Module)> = mods
        .iter()
        .map(|(path, src)| {
            let module = perry_parser::parse_typescript(src, path).expect("parses");
            let name = path.trim_end_matches(".js").trim_end_matches(".ts");
            (path.to_string(), perry_hir::lower_module(&module, name, path).expect("lowers"))
        })
        .collect();
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

const ED: &str = "class EventDispatcher {
  addEventListener(type, listener) { if (this._listeners === undefined) this._listeners = {};
    const listeners = this._listeners; if (listeners[type] === undefined) { listeners[type] = []; }
    if (listeners[type].indexOf(listener) === -1) { listeners[type].push(listener); } }
  dispatchEvent(event) { if (this._listeners === undefined) return; const a = this._listeners[event.type];
    if (a !== undefined) { event.target = this; const c = a.slice(0); for (let i = 0; i < c.length; i++) { c[i].call(this, event); } } } }
";
const CTL: &str = "class Controls extends EventDispatcher { constructor(object, el = null) { super(); this.object = object; this.el = el; this.enabled = true; }
  connect(el) { this.el = el; } }
";
const ORB: &str = "class Orbit extends Controls {
  constructor(object, el = null) { super(object, el); this.enabled = false;
    this.ptrs = []; this.sph = { r: 1 };
    this.onDown = onDown.bind(this);
    if (this.el !== null) { this.connect(this.el); }
    this.update(); }
  update() { this.dispatchEvent({ type: 'change' }); return this.sph.r + this.ptrs.length; } }
function onDown(ev) { if (this.ptrs.length === 0) { this.ptrs.push(ev); } return this.sph.r; }
";
const MAIN: &str = "const o = new Orbit({}, null); o.addEventListener('change', function () { console.log(1); }); o.onDown(1); console.log(o.update());";

fn check(tag: &str, pf: &crate::ProgramFacts, path: &str) -> bool {
    show_class(tag, pf, "Orbit");
    let mut all = true;
    for k in ["ptrs", "sph"] {
        for s in reads(pf, path, k) {
            eprintln!("SB4 {tag} .{k} @{} call={} verdict={:?}", s.0, s.1, s.2);
            all &= proven(&s.2);
        }
    }
    all
}

// S2: S1 plus an EventDispatcher base (no constructor) with listener storage, one module.
#[test]
fn siteb4_s2_dispatcher_one_module() {
    let src = format!("{ED}{CTL}{ORB}{MAIN}");
    let pf = facts_at("a.js", &src);
    assert!(check("s2", &pf, "a.js"));
}

/// Modules `name.ts` with `./name` imports and re-exports resolved
/// (export_star.rs `whole`).
fn whole(mods: &[(&str, String)]) -> crate::ProgramFacts {
    whole_inh(mods, &[])
}

type Inherited<'x> = [(&'x str, Vec<(&'x str, Vec<&'x str>)>)];

fn whole_inh(mods: &[(&str, String)], inherited: &Inherited) -> crate::ProgramFacts {
    let hirs: Vec<(String, Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            // The driver hands a module the field names of the classes it
            // imports (`imported_class_fields`): a constructor store of an
            // inherited field is then no reservation of the child.
            let mut hir = match inherited.iter().find(|(m, _)| m == name) {
                Some((_, fields)) => {
                    let mut by: std::collections::HashMap<String, Vec<(String, perry_hir::types::Type)>> =
                        std::collections::HashMap::new();
                    for (cls, ks) in fields.iter() {
                        by.insert(
                            cls.to_string(),
                            ks.iter().map(|k| (k.to_string(), perry_hir::types::Type::Any)).collect(),
                        );
                    }
                    perry_hir::lower_module_with_class_id_types_and_seed(&module, name, &path, 1000, None, Some(&by))
                        .expect("lowers")
                        .0
                }
                None => perry_hir::lower_module(&module, name, &path).expect("lowers"),
            };
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let mut cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    for (path, hir) in &hirs {
        for e in &hir.exports {
            let (perry_hir::Export::ReExport { source, .. }
            | perry_hir::Export::ExportAll { source }
            | perry_hir::Export::NamespaceReExport { source, .. }) = e
            else {
                continue;
            };
            if let Some(n) = source.strip_prefix("./") {
                cfg.export_sources
                    .entry(path.clone())
                    .or_default()
                    .insert(source.clone(), format!("{n}.ts"));
            }
        }
    }
    let list: Vec<(String, &Module)> = hirs.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(&list, &cfg)
}

// S3 (c09 OrbitControls.js: `extends Controls` imported from the three.js
// barrel): the parent is a value the solve settles. Settled to one class,
// the derived constructor's stores before `this` leaks are definite birth
// fields as for a parent linked by name (S2).
#[test]
fn siteb4_s3_parent_through_barrel() {
    let mods = vec![
        ("core", format!("export {ED}export {CTL}")),
        ("three", "export * from './core';\n".to_string()),
        ("orbit", format!("import {{ Controls }} from './three';\nexport {ORB}")),
        ("main", format!("import {{ Orbit }} from './orbit';\n{MAIN}")),
    ];
    let pf = whole(&mods);
    assert!(check("s3", &pf, "orbit.ts"));
}

// S3b: S3 with the importing modules first (c09 lists OrbitControls.js and
// webgl.ts before the modules their parents come from).
#[test]
fn siteb4_s3b_parent_module_later() {
    let mods = vec![
        ("main", format!("import {{ Orbit }} from './orbit';\n{MAIN}")),
        ("orbit", format!("import {{ Controls }} from './three';\nexport {ORB}")),
        ("three", "export * from './core';\n".to_string()),
        ("core", format!("export {ED}export {CTL}")),
    ];
    let pf = whole(&mods);
    assert!(check("s3b", &pf, "orbit.ts"));
}

// S3c: a direct import, importer first (webgl.ts `extends ContextCore`).
#[test]
fn siteb4_s3c_direct_import_later() {
    let mods = vec![
        ("main", format!("import {{ Orbit }} from './orbit';\n{MAIN}")),
        ("orbit", format!("import {{ Controls }} from './core';\nexport {ORB}")),
        ("core", format!("export {ED}export {CTL}")),
    ];
    let pf = whole(&mods);
    assert!(check("s3c", &pf, "orbit.ts"));
}

// S4 (c09 OrbitControls.js / webgl.ts): S3c with the driver's imported field
// names: `this.enabled` is the parent's field, no reservation of Orbit, so
// before the parent value settles it is a key outside Orbit's layout order.
#[test]
fn siteb4_s4_inherited_field_parent_value() {
    let mods = vec![
        ("main", format!("import {{ Orbit }} from './orbit';\n{MAIN}")),
        ("orbit", format!("import {{ Controls }} from './core';\nexport {ORB}")),
        ("core", format!("export {ED}export {CTL}")),
    ];
    let inh = [("orbit", vec![("Controls", vec!["object", "el", "enabled"])])];
    let pf = whole_inh(&mods, &inh);
    assert!(check("s4", &pf, "orbit.ts"));
    // The stores before `this` leaks are definite birth fields (base: the
    // phase taken before the parent value settled left none of them).
    let t = pf.templates.iter().find(|t| t.display.ends_with(":Orbit")).expect("Orbit");
    for k in ["ptrs", "sph"] {
        let f = t.fields.iter().find(|f| f.key == k).expect("field");
        assert!(f.definite && f.slot.is_some(), "{k}: definite={} slot={:?}", f.definite, f.slot);
    }
    for k in ["ptrs", "sph"] {
        for s in reads(&pf, "orbit.ts", k) {
            assert!(!matches!(s.2, crate::Verdict::OwnDataOrAbsent), ".{k} @{}: {:?}", s.0, s.2);
        }
    }
}

// S1 (OrbitControls.js:96-470): a derived constructor stores its fields after
// super(), binds module functions to `this`, then calls methods; the fields are
// read in a class method and in a bound function.
const S1: &str = "class Controls { constructor(object, el = null) { this.object = object; this.el = el; this.enabled = true; } }
class Orbit extends Controls {
  constructor(object, el = null) { super(object, el); this.enabled = false;
    this.ptrs = []; this.sph = { r: 1 };
    this.onDown = onDown.bind(this);
    if (this.el !== null) { this.connect(this.el); }
    this.update(); }
  connect(el) { this.el = el; }
  update() { return this.sph.r + this.ptrs.length; } }
function onDown(ev) { if (this.ptrs.length === 0) { this.ptrs.push(ev); } return this.sph.r; }
const o = new Orbit({}, null); o.onDown(1); console.log(o.update());";

fn run(tag: &str, path: &str) -> bool {
    let pf = facts_at(path, S1);
    show_class(tag, &pf, "Orbit");
    let mut all = true;
    for k in ["ptrs", "sph"] {
        for s in reads(&pf, path, k) {
            eprintln!("SB4 {tag} .{k} @{} call={} verdict={:?}", s.0, s.1, s.2);
            all &= proven(&s.2);
        }
    }
    all
}

#[test]
fn siteb4_s1_ts() {
    assert!(run("ts", "a.ts"));
}

#[test]
fn siteb4_s1_js() {
    assert!(run("js", "a.js"));
}

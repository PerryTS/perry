//! Entry outlining must preserve module export bindings.
use super::*;
use perry_codegen::codegen::entry_outline::{outline_entry_module, OutlineOutcome};
use perry_hir::{Module, Stmt};
use perry_wpfacts::{FactsConfig, Verdict};

fn lower(path: &str, source: &str) -> Module {
    let ast = perry_parser::parse_typescript(source, path).unwrap();
    perry_hir::lower_module(&ast, path, path).unwrap()
}

fn cjs(path: &str, source: &str) -> String {
    super::super::cjs_wrap::wrap_commonjs_with_addon_paths(
        source,
        Path::new(path),
        None,
        false,
        None,
        None,
        &mut Vec::new(),
        None,
    )
    .0
}

fn check_outlined_exports(require: bool) {
    // The real namespace/export bridge shape, with enough synthetic named
    // exports to trigger production outlining. No outlining env override.
    let names = (0..1000)
        .map(|i| format!("item{i}: () => {i},\n"))
        .collect::<String>();
    let body = format!(
        r#"
var ts = {{}}; ((module) => {{
var __defProp = Object.defineProperty;
var __export = (target, all) => {{
  for (var name in all) __defProp(target, name, {{get: all[name], enumerable: true}});
}};
var out = {{}};
function make() {{ return {{answer: 42}}; }}
__export(out, {{make: () => make, {names}}});
module.exports = out;
}})({{get exports() {{return ts;}}, set exports(v) {{
ts = v; if (typeof module !== "undefined" && module.exports) module.exports = v;
}}}});
"#
    );
    let driver = if require {
        cjs(
            "/fixture/driver.js",
            "const ts = require('./x.cjs'); console.log(ts.make().answer);",
        )
    } else {
        "import ts from './x.cjs'; console.log(ts.make().answer);".into()
    };
    let mut modules = BTreeMap::from([
        (
            PathBuf::from("/fixture/x.cjs"),
            lower("/fixture/x.cjs", &cjs("/fixture/x.cjs", &body)),
        ),
        (
            PathBuf::from("/fixture/driver.js"),
            lower("/fixture/driver.js", &driver),
        ),
    ]);
    let support = super::super::cjs_wrap::support::source(
        &["/fixture/x.cjs".into(), "/fixture/driver.js".into()],
        true,
    );
    modules.insert(
        PathBuf::from("/fixture/support.js"),
        lower("/fixture/support.js", &support),
    );
    for module in modules.values_mut() {
        for imp in &mut module.imports {
            if imp.source == "perry:cjs" {
                imp.resolved_path = Some("/fixture/support.js".into());
            }
            if imp.source == "./x.cjs" {
                imp.resolved_path = Some("/fixture/x.cjs".into());
            }
        }
    }
    let library = modules.get_mut(Path::new("/fixture/x.cjs")).unwrap();
    assert!(matches!(
        outline_entry_module(library),
        OutlineOutcome::Outlined { .. }
    ));
    assert!(
        !library
            .init
            .iter()
            .any(|s| matches!(s, Stmt::Let {name, ..} if name == "_cjs")),
        "the regression must exercise moved module declarations"
    );
    let inputs = facts_inputs(&modules);
    let list = inputs
        .iter()
        .map(|(p, m)| (p.clone(), m.as_ref()))
        .collect::<Vec<_>>();
    let cfg = FactsConfig {
        explain: true,
        max_work: Some(50_000_000),
        ..Default::default()
    };
    // Name resolution is asserted before the per-unit contract boundary.
    // Each mode receives this same input through program_facts::run.
    let facts = perry_wpfacts::compute(&list, &cfg);
    {
        assert!(!facts.stats.aborted, "{}", facts.dump_text());
        let driver_id = facts
            .dump_text()
            .lines()
            .filter_map(|l| l.strip_prefix("M "))
            .find_map(|l| {
                let (id, p) = l.split_once(' ')?;
                (p == "/fixture/driver.js").then(|| id.parse::<u32>().unwrap())
            })
            .unwrap();
        let sites = facts
            .sites
            .iter()
            .filter(|s| s.module == driver_id && s.key == "answer")
            .collect::<Vec<_>>();
        assert_eq!(sites.len(), 1);
        assert!(
            matches!(sites[0].verdict, Verdict::T0Mono | Verdict::T0Set),
            "{}",
            facts.dump_text()
        );
    }
    // Analysis cannot mutate emitted functions, calls or site coordinates.
    assert!(!modules[Path::new("/fixture/x.cjs")]
        .init
        .iter()
        .any(|s| matches!(s, Stmt::Let {name, ..} if name == "_cjs")));
}

#[test]
fn outlined_cjs_default_import_keeps_its_module_bindings() {
    check_outlined_exports(false);
}
#[test]
fn outlined_cjs_require_keeps_its_module_bindings() {
    check_outlined_exports(true);
}

#[test]
fn facts_input_preserves_an_outlined_factory_scope() {
    let body = (0..1100)
        .map(|i| format!("var item{i} = {i};\n"))
        .collect::<String>()
        + "module.exports = function () { return item1099; };";
    let mut module = lower("/fixture/factory.js", &cjs("/fixture/factory.js", &body));
    assert!(matches!(
        outline_entry_module(&mut module),
        OutlineOutcome::Outlined { .. }
    ));
    let modules = BTreeMap::from([(PathBuf::from("/fixture/factory.js"), module)]);
    let inputs = facts_inputs(&modules);
    assert!(
        matches!(inputs[0].1, std::borrow::Cow::Borrowed(_)),
        "a chunk called inside a factory belongs to the factory, not module scope"
    );
    assert!(!inputs[0]
        .1
        .init
        .iter()
        .any(|s| matches!(s,Stmt::Let{name,..} if name=="item1099")));
}

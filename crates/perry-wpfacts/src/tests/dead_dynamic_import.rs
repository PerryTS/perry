//! A dynamic `import()` held only by a body that cannot run (DCE layer 2).
//! `defaultFormat` is the destructuring default of `generateTypescript`
//! options: every caller passes `format`, so the default is never taken, no
//! body calls `defaultFormat`, `loadFmt` never runs and the `import()` of the
//! formatter never happens (postgrest-typegen and oxfmt in the Supabase CLI).
//! Each arm that can take the default keeps the import live.

use super::*;

const FMT: &str = r#"
export async function format(name: string, code: string, opts: any) { return { code: "F:" + code, errors: [] as any[] }; }
"#;

const GEN: &str = r#"
const loadFmt = async () => {
  try { return await import("./fmtlib.js"); }
  catch (error) { throw new Error("not installed", { cause: error }); }
};
const defaultFormat = async (code: string) => {
  const { format } = await loadFmt();
  const { code: formatted } = await format("output.ts", code, { semi: false });
  return formatted;
};
export const generateTypescript = async (metadata: any, opts: any = {}) => {
  const { detect = false, defaultSchema = "public", format = defaultFormat } = opts;
  let output = "types for " + metadata.name + " " + defaultSchema + " " + detect;
  output = await format(output);
  return output;
};
"#;

const LANG: &str = r#"
import { generateTypescript } from "./gen";
export const typescript = {
  name: "typescript",
  async generate(metadata: any, values: any, host: any) {
    const { format } = host;
    return generateTypescript(metadata, {
      detect: values.detect === true,
      defaultSchema: values.schema as string,
      ...(format ? { format: (code: string) => format(code, "output.ts") } : {}),
    });
  },
};
"#;

fn main_with(host: &str) -> String {
    format!(
        r#"
import {{ typescript }} from "./lang";
const host = {host};
typescript.generate({{ name: "db" }}, {{ detect: process.argv.length > 5, schema: "public" }}, host).then((s: string) => console.log(s));
"#
    )
}

/// The program as the compile driver hands it over: the `import()` site's
/// `paths` filled, its target a dynamic import entry, relative sources
/// resolved to `<name>.ts`.
fn hirs(main: &str) -> Vec<(String, perry_hir::Module)> {
    [
        ("fmtlib", FMT),
        ("gen", GEN),
        ("lang", LANG),
        ("main", main),
    ]
    .iter()
    .map(|(name, src)| {
        let path = format!("{name}.ts");
        let module = perry_parser::parse_typescript(src, &path).expect("parses");
        let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
        let mut sites = 0;
        perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
            if let perry_hir::Expr::DynamicImport { paths: p, .. } = e {
                *p = vec!["./fmtlib.js".to_string()];
                sites += 1;
            }
        });
        if sites > 0 {
            hir.imports.push(perry_hir::Import {
                source: "./fmtlib.js".to_string(),
                specifiers: Vec::new(),
                is_native: false,
                module_kind: Default::default(),
                resolved_path: None,
                type_only: false,
                runtime_erased: false,
                is_dynamic: true,
                is_dynamic_target: false,
                is_deferred_require: false,
                is_adopted_require: false,
            });
        }
        for imp in &mut hir.imports {
            if let Some(n) = imp.source.strip_prefix("./") {
                imp.resolved_path = Some(format!("{}.ts", n.trim_end_matches(".js")));
            }
        }
        (path, hir)
    })
    .collect()
}

/// Is the formatter's `import()` held by a body that can run?
fn imports_formatter(host: &str) -> bool {
    let hirs = hirs(&main_with(host));
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let live = crate::live_dynamic_imports(&list, &FactsConfig::default()).expect("in bound");
    live.contains(&("gen.ts".to_string(), "./fmtlib.js".to_string()))
}

const CLI_HOST: &str =
    "{ cwd: \"/\", format: (code: string, _n: string) => Promise.resolve(code) }";

#[test]
fn a_default_every_caller_overrides_never_imports() {
    assert!(!imports_formatter(CLI_HOST));
    // The whole program agrees: neither `loadFmt` nor `defaultFormat` nor
    // the formatter can run.
    let hirs = hirs(&main_with(CLI_HOST));
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let pf = crate::compute(&list, &FactsConfig::default());
    let on = pf.active_bodies.as_ref().expect("activated");
    for (path, id) in [("gen.ts", 0), ("gen.ts", 1), ("fmtlib.ts", 0)] {
        assert!(
            !on.iter().any(|(p, i, _)| p == path && *i == id),
            "{path} fn {id} active"
        );
    }
}

#[test]
fn every_way_the_default_can_be_taken_imports() {
    for host in [
        // No `format` at all.
        "{ cwd: \"/\" }",
        // `format` may be `undefined`.
        "{ cwd: \"/\", format: process.argv.length > 9 ? (c: string) => Promise.resolve(c) : undefined }",
        // `format` may be falsy (the spread copies `{}`).
        "{ cwd: \"/\", format: process.argv.length > 9 ? (c: string) => Promise.resolve(c) : 0 }",
        // The key is deleted before the call.
        "(() => { const h: any = { cwd: \"/\", format: (c: string) => Promise.resolve(c) }; if (process.argv.length > 9) delete h.format; return h; })()",
        // The host comes from unknown code.
        "(globalThis as any)[\"ho\" + \"st\"]",
    ] {
        assert!(imports_formatter(host), "{host}");
    }
}

/// Is `oxmark` able to run in `src`?
fn oxmark_runs(src: &str) -> bool {
    let src = format!("function oxmark(c: string) {{ return \"OX\" + c; }}\nconst d = (c: string) => oxmark(c);\n{src}");
    let m = perry_parser::parse_typescript(&src, "p.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "p", "p.ts").expect("lowers");
    let pf = crate::compute(&[("p.ts".to_string(), &hir)], &FactsConfig::default());
    let ox = hir
        .functions
        .iter()
        .find(|f| f.name == "oxmark")
        .expect("oxmark")
        .id;
    pf.active_bodies
        .as_ref()
        .expect("activated")
        .iter()
        .any(|(_, id, _)| *id == ox)
}

#[test]
fn a_branch_runs_only_when_its_test_can_select_it() {
    let call = "console.log(g({ format: (c: string) => c }));";
    for (src, runs) in [
        // Conditionals, logical operators, destructuring and parameter
        // defaults over a value that is always a function.
        ("const g = (o: any) => { const f = o.format; return (f === undefined ? d : f)(\"x\"); };", false),
        ("const g = (o: any) => { const { format = d } = o; return format(\"x\"); };", false),
        ("const g = async (o: any = {}) => { const { format = d } = o; return await format(\"x\"); };", false),
        ("const g = (o: any) => (o.format || d)(\"x\");", false),
        ("const g = (o: any) => (o.format ?? d)(\"x\");", false),
        ("const g = (o: any) => { let f = o.format; if (f === undefined) { f = d; } return f(\"x\"); };", false),
        // Each test that can select the branch.
        ("const g = (o: any) => (o.missing || d)(\"x\");", true),
        ("const g = (o: any) => (o.format && d)(\"x\");", true),
        ("const g = (o: any) => { let f = o.missing; if (f === undefined) { f = d; } return f(\"x\"); };", true),
        ("const g = (o: any) => { let f = o.format; if (f !== undefined) { f = d; } return f(\"x\"); };", true),
    ] {
        assert_eq!(oxmark_runs(&format!("{src}\n{call}")), runs, "{src}");
    }
    // A default the caller omits, an option a spread may not copy.
    assert!(oxmark_runs(
        "const g = (o: any = { format: d }) => o.format(\"x\"); console.log(g());"
    ));
    assert!(oxmark_runs(
        "const g = (o: any) => { const { format = d } = o; return format(\"x\"); };\nconst k: any = process.argv.length > 9 ? { format: (c: string) => c } : {};\nconsole.log(g({ ...k }));"
    ));
}

/// Modules `(name, source)`, every relative `import()` site filled with its
/// specifier and its target a dynamic import entry, as the driver hands them.
fn program_of(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            let mut specs = Vec::new();
            perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
                if let perry_hir::Expr::DynamicImport { paths, arg, .. } = e {
                    if let perry_hir::Expr::String(s) = arg.as_ref() {
                        *paths = vec![s.clone()];
                        specs.push(s.clone());
                    }
                }
            });
            for s in specs {
                hir.imports.push(perry_hir::Import {
                    source: s,
                    specifiers: Vec::new(),
                    is_native: false,
                    module_kind: Default::default(),
                    resolved_path: None,
                    type_only: false,
                    runtime_erased: false,
                    is_dynamic: true,
                    is_dynamic_target: false,
                    is_deferred_require: false,
                    is_adopted_require: false,
                });
            }
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{}.ts", n.trim_end_matches(".js")));
                }
            }
            (path, hir)
        })
        .collect()
}

const LAZY: &str = r#"
// Its body runs only when it loads; it writes a computed key of `Object`.
(Object as any)[(globalThis as any)["k" + "ey"]] = (x: any) => x;
export const v = 1;
"#;

#[test]
fn a_module_only_an_uncalled_body_imports_never_runs() {
    for (main, loads) in [
        ("const never = () => import(\"./lazy.js\");\nconsole.log([1, 2].map((x: number) => x + 1), typeof never);", false),
        ("const now = () => import(\"./lazy.js\");\nnow().then((m: any) => console.log(m.v, [1].map((x: number) => x)));", true),
    ] {
        let hirs = program_of(&[("lazy", LAZY), ("main", main)]);
        let list: Vec<(String, &perry_hir::Module)> = hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
        let live = crate::live_dynamic_imports(&list, &FactsConfig::default()).expect("in bound");
        assert_eq!(live.contains(&("main.ts".to_string(), "./lazy.js".to_string())), loads, "{main}");
        let pf = crate::compute(&list, &FactsConfig::default());
        let on = pf.active_bodies.as_ref().expect("activated");
        let init = on.iter().any(|(p, id, name)| p == "lazy.ts" && *id == u32::MAX && name.ends_with(":<init>"));
        assert_eq!(init, loads, "{main}");
    }
}

#[test]
fn a_root_runs_whatever_imports_it() {
    // A worker entry: the driver's dynamic edge names it, no `import()` site
    // does; its body runs in the worker (`FactsConfig::roots`).
    let worker = "(Object as any)[(globalThis as any)[\"k\" + \"ey\"]] = 1; export const w = 1;";
    let mut hirs = program_of(&[
        ("lazy", worker),
        ("main", "console.log([1].map((x: number) => x));"),
    ]);
    hirs[1].1.imports.push(perry_hir::Import {
        source: "./lazy.ts".to_string(),
        specifiers: Vec::new(),
        is_native: false,
        module_kind: Default::default(),
        resolved_path: Some("lazy.ts".to_string()),
        type_only: false,
        runtime_erased: false,
        is_dynamic: true,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    });
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    for (roots, runs) in [(vec![], false), (vec!["lazy.ts".to_string()], true)] {
        let cfg = FactsConfig {
            roots,
            ..FactsConfig::default()
        };
        let pf = crate::compute(&list, &cfg);
        let on = pf.active_bodies.as_ref().expect("activated");
        let init = on
            .iter()
            .any(|(p, id, name)| p == "lazy.ts" && *id == u32::MAX && name.ends_with(":<init>"));
        assert_eq!(init, runs);
    }
}

/// `X.prototype.method` read as a value is a builtin function (the token
/// stands for every builtin prototype): a callback given to it through
/// `call`, `bind` or a call-bound helper runs (get-intrinsic's
/// `stringToPath`: `$replace = callBind(String.prototype.replace)`).
#[test]
fn a_callback_given_through_a_builtin_prototype_method_value_runs() {
    for src in [
        "const $replace: any = Function.prototype.call.bind(String.prototype.replace);\nconst r: any[] = [];\n$replace(\"aa\", /a/g, (m: any) => { r[r.length] = d; return m; });\nconsole.log(r[0](\"x\"));",
        "const $call: any = Function.prototype.call;\nconst r: any[] = [];\n$call.call(String.prototype.replace, \"aa\", /a/g, (m: any) => { r[r.length] = d; return m; });\nconsole.log(r[0](\"x\"));",
        "const t: any = String.prototype.replace;\nt.call(\"aa\", /a/g, d);",
        "const $replace: any = String.prototype.replace.bind(\"aa\");\n$replace(/a/g, d);",
        // The callback's parameters are the native's: a guard over them
        // lets either branch through.
        "const $replace: any = Function.prototype.call.bind(String.prototype.replace);\nconst r: any[] = [];\n$replace(\"aa\", /a/g, (m: any, n: any) => { r[r.length] = n ? d : d; return m; });\nconsole.log(r[0](\"x\"));",
    ] {
        assert!(oxmark_runs(src), "{src}");
    }
}

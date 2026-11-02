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

/// `(name, source, import() targets)` as the compile driver hands them: each
/// `import()` site in order gets its target, the target a dynamic import
/// entry, relative sources resolved to `<name>.ts`.
fn program(mods: &[(&str, &str, &[&str])]) -> Vec<(String, perry_hir::Module)> {
    mods.iter()
        .map(|(name, src, targets)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            let mut next = targets.iter();
            perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
                if let perry_hir::Expr::DynamicImport { paths: p, .. } = e {
                    *p = vec![next.next().expect("a target per site").to_string()];
                }
            });
            for t in *targets {
                hir.imports.push(perry_hir::Import {
                    source: t.to_string(),
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

#[test]
fn a_re_export_source_loads_with_the_module_that_re_exports_it() {
    // main loads e by `import()`; e re-exports from package b, which the
    // driver resolves (`export_sources`; @supabase/typegen re-exporting its
    // languages); b imports c at module scope. b loads with e, so its
    // `import()` runs.
    let hirs = program(&[
        (
            "b",
            "export const f = 1;\nimport(\"./c.js\").then((m: any) => console.log(m.v));\n",
            &["./c.js"],
        ),
        ("c", "export const v = 2;\n", &[]),
        ("e", "export { f } from \"b-pkg\";\n", &[]),
        (
            "main",
            "import(\"./e.js\").then((m: any) => console.log(m.f));\n",
            &["./e.js"],
        ),
    ]);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let mut cfg = FactsConfig::default();
    cfg.export_sources
        .entry("e.ts".to_string())
        .or_default()
        .insert("b-pkg".to_string(), "b.ts".to_string());
    let live = crate::live_dynamic_imports(&list, &cfg).expect("in bound");
    assert!(
        live.contains(&("main.ts".to_string(), "./e.js".to_string())),
        "{live:?}"
    );
    assert!(
        live.contains(&("b.ts".to_string(), "./c.js".to_string())),
        "{live:?}"
    );
}

/// Effect's schema `make` (effect/dist/internal/schema/make.js): a function
/// object whose chain is set to a prototype, then given own `name` /
/// `length` through `Object.defineProperties`.
const MAKE: &str = r#"
const TypeId = "~effect/Schema/Schema";
const RebuildOptions = Symbol();
const SchemaProto: any = {
  [TypeId]: TypeId,
  get make() { const value = 1; Object.defineProperty(this, "make", { value, enumerable: true }); return value; },
  pipe() { return this; },
  rebuild(ast: any) { return make(ast, this[RebuildOptions]); }
};
export function make(ast: any, options: any) {
  function Schema() {}
  const self: any = Object.setPrototypeOf(Schema, SchemaProto);
  if (options && (Object.hasOwn(options, "name") || Object.hasOwn(options, "length") || Object.hasOwn(options, "__proto__"))) {
    Object.defineProperties(self, Object.getOwnPropertyDescriptors({ ...options }));
  } else {
    Object.assign(self, options);
  }
  self[RebuildOptions] = options;
  self.ast = ast;
  return self;
}
"#;

/// The typegen program plus `mk.ts` (`MAKE` + `extra`), with `main_tail`
/// before the `languages.map(...)` call. With `interp`, the program holds
/// interpreted code that names globals, so every namespace call is lowered
/// guarded (`NamespacePatched(ns) ? globalThis.ns.m(..) : intrinsic`) and the
/// facts know interpreted code may run. Is the formatter's `import()` live?
fn imports_formatter_with_make(extra: &str, main_tail: &str, interp: bool) -> bool {
    imports_formatter_with_mods(extra, main_tail, interp, &[])
}

/// `imports_formatter_with_make` with more program modules (`name`, source),
/// each at `<name>.ts`.
fn imports_formatter_with_mods(
    extra: &str,
    main_tail: &str,
    interp: bool,
    mods: &[(&str, &str)],
) -> bool {
    let mut reach = perry_hir::dynamic_code::DynamicCodeReach::default();
    if interp {
        reach.record_global("new Function(...) @ x.ts (byte 1) (names globals)".into());
    }
    let main = format!(
        "import {{ typescript }} from \"./lang\";\nimport {{ make }} from \"./mk\";\nconst host = {CLI_HOST};\n{main_tail}\n[typescript].map((l: any) => l.generate({{ name: \"db\" }}, {{ detect: process.argv.length > 5, schema: \"public\" }}, host)).forEach((p: any) => p.then((s: string) => console.log(s)));\n"
    );
    let mut hirs = hirs(&main);
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    pb.dynamic_code_reach = reach.clone();
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let m = perry_parser::parse_typescript(&format!("{MAKE}{extra}"), "mk.ts").expect("parses");
    let h = perry_hir::lower_module(&m, "mk", "mk.ts").expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    hirs.push(("mk.ts".to_string(), h));
    for (name, src) in mods {
        let path = format!("{name}.ts");
        let m = perry_parser::parse_typescript(src, &path).expect("parses");
        let mut h = perry_hir::lower_module(&m, name, &path).expect("lowers");
        // A literal `import("./x")` of another listed module: its target, as
        // `collect_modules` folds it.
        let mut dyn_targets = Vec::new();
        perry_hir::for_each_dynamic_import_mut(&mut h, &mut |e| {
            if let perry_hir::Expr::DynamicImport { paths, arg, .. } = e {
                if let perry_hir::Expr::String(s) = arg.as_ref() {
                    *paths = vec![s.clone()];
                    dyn_targets.push(s.clone());
                }
            }
        });
        for source in dyn_targets {
            h.imports.push(perry_hir::Import {
                source,
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
        hirs.push((path.clone(), h));
    }
    for (_, h) in hirs.iter_mut() {
        for imp in &mut h.imports {
            if imp.source == "./mk" {
                imp.resolved_path = Some("mk.ts".into());
            } else if let Some(n) = imp.source.strip_prefix("./") {
                if mods.iter().any(|(m, _)| *m == n) {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
        }
    }
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let mut cfg = FactsConfig::default();
    // `export { x } from "./m"` reads `m.ts`, as the driver resolves it.
    for (path, h) in &hirs {
        for e in &h.exports {
            if let perry_hir::Export::ReExport { source, .. } = e {
                if let Some(n) = source.strip_prefix("./") {
                    cfg.export_sources
                        .entry(path.clone())
                        .or_default()
                        .insert(source.clone(), format!("{n}.ts"));
                }
            }
        }
    }
    if interp {
        cfg.dynamic_code = Some("interp".into());
        cfg.dynamic_code_reach = reach;
    }
    let live = crate::live_dynamic_imports(&list, &cfg).expect("in bound");
    live.contains(&("gen.ts".to_string(), "./fmtlib.js".to_string()))
}

const MAKE_CALLED: &str = "console.log(make({ k: 1 }, { name: \"n\" }));";

/// Rule 1: the generic arm of a guarded `Object.setPrototypeOf(f, P)` is the
/// intrinsic's operation (E6 on `f`, `f`'s chain holds `P`, the result is
/// `f`), not an unknown call whose result is ⊤. Otherwise `self` is ⊤, the
/// `Object.defineProperties(self, ..)` is an any-key write on ⊤, every
/// builtin key may be written and the language objects reach unknown code.
#[test]
fn a_guarded_set_prototype_of_returns_its_target() {
    assert!(!imports_formatter_with_make("", MAKE_CALLED, true));
    assert!(!imports_formatter_with_make("", MAKE_CALLED, false));
    // The fixture can fail: an unknown prototype written into `self` makes
    // the store an any-key write on ⊤ again.
    assert!(imports_formatter_with_make(
        "export function poke(v: any) { const s: any = Object.setPrototypeOf(v, (globalThis as any)[String(process.argv.length)]); Object.defineProperties(s.x, Object.getOwnPropertyDescriptors({ ...v })); }",
        &format!("{MAKE_CALLED}\nimport {{ poke }} from \"./mk\";\npoke(process.argv);"),
        true
    ));
}

/// Rule 3: an operand of a construct the model does not follow (here the
/// runtime's `crypto.getRandomValues` fill, effect's Random.js
/// `getInitialSeed`) is the value handed over, not the prototypes on its
/// chain: those are reached along the chain (`Model::chain_sinks`), as the
/// prototypes of an escaped value are, so no builtin prototype is written.
#[test]
fn an_unmodeled_operand_does_not_hand_over_its_prototypes() {
    let seed = "const seed = crypto.getRandomValues(new Uint32Array(2)); console.log(seed[0]);";
    assert!(!imports_formatter_with_make("", seed, false));
    assert!(!imports_formatter_with_make("", seed, true));
}

// CJS3 exploration (temporary)
const PGT: &str = r#"
const typeParsers: any = { text: {}, binary: {} };
export function setTypeParser(oid: any, format: any, parseFn?: any) {
  if (typeof format == "function") { parseFn = format; format = "text"; }
  typeParsers[format][oid] = parseFn;
}
"#;
const YREV: &str = r#"
export function applyReviver(reviver: any, obj: any, key: any, val: any): any {
  if (val && typeof val === "object") {
    if (Array.isArray(val)) {
      for (let i = 0, len = val.length; i < len; ++i) {
        const v0 = val[i]; const v1 = applyReviver(reviver, val, String(i), v0);
        if (v1 === undefined) delete val[i]; else if (v1 !== v0) val[i] = v1;
      }
    } else {
      for (const [k, v0] of Object.entries(val)) {
        const v1 = applyReviver(reviver, val, k, v0);
        if (v1 === undefined) delete val[k]; else if (v1 !== v0) val[k] = v1;
      }
    }
  }
  return reviver.call(obj, key, val);
}
"#;
/// Object.entries/values read own properties only (EnumerableOwnProperties),
/// and a `JSON.parse` object is fresh data: its own properties are its
/// modeled fields. yaml's `applyReviver` deletes `val[k]` for the keys
/// `Object.entries(val)` gave; without the two rules `val` holds
/// Object.prototype's builtin methods (inherited, never an entry) or ⊤ (own
/// values of an opaque builtin object), the delete is an any-key E7 on a
/// native prototype or on ⊤, and the language objects reach unknown code.
#[test]
fn object_entries_of_parsed_json_are_its_own_data() {
    let tail = r#"import { applyReviver } from "./mk";
const r = applyReviver((k: any, v: any) => v, { "": JSON.parse(process.argv[2]) }, "", JSON.parse(process.argv[2]));
process.exitCode = r === undefined ? 1 : 0;"#;
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_make(YREV, tail, interp),
            "interp={interp}: Object.entries of JSON.parse data keeps the formatter import live"
        );
    }
}

/// @supabase/auth-js `polyfillGlobalThis` ends with `delete
/// Object.prototype.__magic__`: a delete of one constant key of a native
/// prototype writes that key and no other. Noted under the key, never as an
/// any-key write (which makes every builtin key written: the options' reads
/// in the generator turn opaque and the language objects reach unknown code).
const POLYFILL: &str = r#"
export function polyfillGlobalThis(): void {
  delete (Object.prototype as any).__magic__;
}
"#;
#[test]
fn a_keyed_delete_on_a_native_prototype_writes_its_key() {
    let tail = "import { polyfillGlobalThis } from \"./mk\";\npolyfillGlobalThis();";
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_make(POLYFILL, tail, interp),
            "interp={interp}: delete Object.prototype.__magic__ keeps the formatter import live"
        );
    }
}

/// Rule 6: a computed key that is an import of a module's `const` string
/// binding is that string, through re-exports (an import of a binding is
/// the binding). effect's `FiberImpl.runLoop` saves and restores
/// `globalThis[currentFiberTypeId]` (`currentFiberTypeId` a `const` string
/// of Redactable.js); as an any-key store it writes every field of
/// globalThis, `globalThis.Object` holds the unknown value read back, every
/// guarded `globalThis.Object.X(..)` arm calls it, `make`'s `self` is that
/// value and its `Object.defineProperties` is an any-key write on it.
const FIBER_KEY: &str = r#"export const currentFiberTypeId = "~effect/Fiber/currentFiber";
"#;
const FIBER_REEXPORT: &str = r#"export { currentFiberTypeId as fiberKey } from "./rdk";
"#;
const RUN_LOOP: &str = r#"
import { fiberKey } from "./rdx";
export function runLoop(fiber: any) {
  const prevFiber = (globalThis as any)[fiberKey];
  (globalThis as any)[fiberKey] = fiber;
  (globalThis as any)[fiberKey] = prevFiber;
}
"#;
#[test]
fn an_imported_const_key_is_its_string() {
    let tail = format!(
        "{MAKE_CALLED}\nimport {{ runLoop }} from \"./mk\";\nrunLoop(require(String(process.argv.length)));"
    );
    let mods = [("rdk", FIBER_KEY), ("rdx", FIBER_REEXPORT)];
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_mods(RUN_LOOP, &tail, interp, &mods),
            "interp={interp}: globalThis[importedConstKey] = v keeps the formatter import live"
        );
    }
    // A module-scope `const` read in a function is TDZ-checked first
    // (`Sequence([check, x])`): its value is still the string.
    let local = format!(
        "const fk = \"~k\";\n{}",
        RUN_LOOP.replace("[fiberKey]", "[fk]")
    );
    assert!(!imports_formatter_with_mods(&local, &tail, true, &mods));
    // The fixture can fail: a key that is not a constant is an any-key store.
    let loose = RUN_LOOP.replace("[fiberKey]", "[String(fiber)]");
    assert!(imports_formatter_with_mods(&loose, &tail, true, &mods));
}

/// Rule 7: effect's core.js `Error` is `function () { return class Base
/// extends YieldableError { .. } }()`: the class's `extends` is evaluated
/// by the class expression, in a closure whose body is walked during the
/// solve, after generation ends. That evaluation is the parent
/// (`feed_parent`); the class was taken as one with an unresolved `extends`
/// (parent ⊤) because no evaluation had fed it when generation ended, so
/// `this` in `Base`'s constructor was ⊤ and its `Object.defineProperty`
/// an any-key write on ⊤.
const CORE: &str = r#"
export const YieldableError = /*#__PURE__*/function () {
  class YieldableError extends globalThis.Error {}
  return YieldableError;
}();
export const Error = /*#__PURE__*/function () {
  const plainArgsSymbol = Symbol.for("effect/Data/Error/plainArgs");
  return class Base extends YieldableError {
    constructor(args: any) {
      super(args?.message, args?.cause ? { cause: args.cause } : undefined);
      if (args) { Object.defineProperty(this, plainArgsSymbol, { value: args, enumerable: false }); }
    }
  };
}();
"#;
#[test]
fn a_class_expression_parent_evaluated_later_is_its_parent() {
    for tail in [
        "import { Error as E } from \"./core\";\nconsole.log(new E({ message: \"m\" }));",
        "import { Error as E } from \"./core\";\nclass T extends E { _tag = \"t\"; }\nconsole.log(new T({ message: \"m\" }));",
    ] {
        for interp in [false, true] {
            assert!(
                !imports_formatter_with_mods("", &format!("{MAKE_CALLED}\n{tail}"), interp, &[("core", CORE)]),
                "interp={interp}: {tail}"
            );
        }
    }
    // The fixture can fail: an unknown parent makes `this` unknown.
    let unknown = CORE.replace(
        "extends YieldableError",
        "extends (require(String(args)) as any)",
    );
    assert!(imports_formatter_with_mods(
        "",
        &format!("{MAKE_CALLED}\nimport {{ Error as E }} from \"./core\";\nconsole.log(new E({{ message: \"m\" }}));"),
        false,
        &[("core", &unknown)]
    ));
}

/// Rule 8: the generic arm of a guarded `Object.create(p)` is the
/// intrinsic's operation (a fresh object of the site whose chain holds `p`),
/// not an unknown call whose result is ⊤. effect's ConfigProvider
/// `buildEnvTrie` (`node.children ??= Object.create(null); node =
/// children[seg] ??= {}`): with a ⊤ result the computed store is an any-key
/// write on ⊤, every builtin key may be written and the language objects
/// reach unknown code.
const CREATE: &str = r#"
export function child(seg: string) {
  const children: any = Object.create(null);
  children[seg] ??= {};
  return children;
}
"#;
#[test]
fn a_guarded_object_create_is_a_fresh_object() {
    let tail = format!("{MAKE_CALLED}\nimport {{ child }} from \"./mk\";\nconsole.log(child(String(process.argv.length)));");
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_make(CREATE, &tail, interp),
            "interp={interp}: a guarded Object.create keeps the formatter import live"
        );
    }
    // The fixture can fail: an unknown object in `children` makes the
    // computed store an any-key write on ⊤ again.
    let unknown = CREATE.replace(
        "Object.create(null)",
        "(globalThis as any)[String(process.argv.length)]",
    );
    assert!(imports_formatter_with_make(&unknown, &tail, true));
}

/// Rule 7 for a member `extends` (@effect/platform-node's
/// `NodeHttpIncomingMessage extends Inspectable.Class`): the HIR keeps the
/// trailing name (`Class`) and the evaluated member; the name binds nothing
/// in the module, the evaluation is the parent. The module is loaded only by
/// `import()` (DECISIONS 225): its body, and with it the evaluation of the
/// parent, is walked when an active body loads it, during the solve, after
/// `feed_parent_values`. With the parent ⊤, `this` in the constructor was ⊤
/// and `this[k] = ..` an any-key store on ⊤.
const INSP: &str = r#"
export class Class { toString() { return "x"; } }
"#;
const MSG: &str = r#"
import * as Inspectable from "./insp";
export class Msg extends Inspectable.Class {
  source: any;
  constructor(source: any) { super(); (this as any)[String(source)] = 1; this.source = source; }
}
"#;
const LOADER: &str = r#"
export async function load(n: number) { const { Msg } = await import("./msg"); return new Msg(n); }
"#;
#[test]
fn a_member_extends_evaluated_is_its_parent() {
    let tail = format!("{MAKE_CALLED}\nimport {{ load }} from \"./loader\";\nload(process.argv.length).then((m: any) => console.log(m));");
    let mods = [("insp", INSP), ("msg", MSG), ("loader", LOADER)];
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_mods("", &tail, interp, &mods),
            "interp={interp}: a member `extends` keeps the formatter import live"
        );
    }
    // The fixture can fail: an unknown parent makes `this` unknown.
    let unknown = MSG.replace(
        "extends Inspectable.Class",
        "extends (require(String(process.argv.length)) as any).Class",
    );
    let mods = [
        ("insp", INSP),
        ("msg", unknown.as_str()),
        ("loader", LOADER),
    ];
    assert!(imports_formatter_with_mods("", &tail, false, &mods));
}

/// Rule 9: `const EventEmitter = require("events").EventEmitter` binds the
/// native module's export as `const { EventEmitter } = require("events")`
/// does (HIR provenance, #10623), so `class Client extends EventEmitter` is
/// the native parent (pg's `Client` and `Connection`). Before, the class was
/// a user shadow whose evaluated parent is the native module ref (⊤):
/// `this` in the constructor was ⊤, every `this.k = ..` a named store into
/// ⊤, and a later `c.connection` read through a native prototype under that
/// key an opaque member (`delete this.connection.s[k]`: E7 on ⊤).
const PGC: &str = r#"
const EventEmitter = require("events").EventEmitter;
export class Client extends EventEmitter {
  connection: any;
  constructor(c: any) { super(); this.connection = c.connection || { s: {} as any }; }
  drop(k: string) { delete this.connection.s[k]; }
}
"#;
#[test]
fn a_required_native_member_parent_is_the_native_parent() {
    native_member_parent_is_the_native_parent(PGC);
}
/// The CJS wrap's form of the same module (Supabase's real pg): the wrap
/// adopts `require("events")` as a default import, so the body reads
/// `const EventEmitter = _req_0.EventEmitter`.
#[test]
fn a_wrapped_required_native_member_parent_is_the_native_parent() {
    let wrapped = PGC.replace(
        "const EventEmitter = require(\"events\").EventEmitter;",
        "import _req_0 from \"events\";\nconst EventEmitter = _req_0.EventEmitter;",
    );
    assert!(wrapped.contains("_req_0.EventEmitter"));
    native_member_parent_is_the_native_parent(&wrapped);
}
fn native_member_parent_is_the_native_parent(src: &str) {
    let m = perry_parser::parse_typescript(src, "cl.ts").expect("parses");
    let h = perry_hir::lower_module(&m, "cl", "cl.ts").expect("lowers");
    let c = h
        .classes
        .iter()
        .find(|c| c.name == "Client")
        .expect("class");
    assert_eq!(
        c.native_extends,
        Some(("events".to_string(), "EventEmitter".to_string()))
    );
    assert!(c.extends_expr.is_none());
    let tail = format!("{MAKE_CALLED}\nimport {{ Client }} from \"./cl\";\nconst c = new Client({{}});\nc.drop(String(process.argv.length));");
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_mods("", &tail, interp, &[("cl", src)]),
            "interp={interp}: a required native parent keeps the formatter import live"
        );
    }
    // The fixture can fail: a reassignable binding is a user value, the
    // evaluated parent is the module ref (⊤).
    let mutable = src.replace("const EventEmitter", "let EventEmitter");
    assert!(imports_formatter_with_mods(
        "",
        &tail,
        false,
        &[("cl", &mutable)]
    ));
}

/// Rule 10 (pg `Client`: `this._handleErrorMessage.bind(this)` handed to
/// code outside the program): a bound function passed there exposes neither its
/// target nor its bound receiver (ES 10.4.1: internal slots); that code may
/// only call it, which runs the target with the bound `this`, or construct
/// it. Before, the escape sank the target, so `this` inside it was ⊤ and
/// `delete this.own[k]` an any-key delete on ⊤. (A described EventEmitter
/// method no longer hands its listener out: rule 11.)
const BOUND_CLI: &str = r#"
import { EventEmitter } from "events";
export class Conn extends EventEmitter { named: any = {}; }
export class Cli extends EventEmitter {
  connection: any; own: any = {};
  constructor() { super(); this.connection = new Conn(); const con = this.connection; (globalThis as any)[String(process.argv.length)]("err", this.onErr.bind(this)); }
  onErr(msg: any) { delete this.own[String(msg)]; }
}
"#;
#[test]
fn an_escaped_bound_method_keeps_its_bound_receiver() {
    let tail = format!("{MAKE_CALLED}\nimport {{ Cli }} from \"./cli\";\nconst c = new Cli();\nconsole.log(c.own);");
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_mods("", &tail, interp, &[("cli", BOUND_CLI)]),
            "interp={interp}: an escaped bound method keeps the formatter import live"
        );
    }
    // The fixture can fail: bound to an unknown receiver, `this` is ⊤ and
    // the delete is an any-key delete on ⊤ again.
    let unknown = BOUND_CLI.replace(
        "this.onErr.bind(this)",
        "this.onErr.bind((globalThis as any)[String(process.argv.length)])",
    );
    assert!(imports_formatter_with_mods("", &tail, false, &[("cli", unknown.as_str())]));
}
/// Rule 11: a described `EventEmitter.prototype` method called on a native
/// subclass instance (`con.on(ev, f)`) is Node's protocol on that receiver
/// (the listener is stored on it, `emit` calls the stored listeners with
/// this = receiver): the receiver does not escape because the method is
/// native. Before, the chain value held the opaque builtin function, whose
/// call escaped `con`; its fields were ⊤ and pg's `delete
/// this.connection.submittedNamedStatements[name]` an any-key delete on ⊤.
#[test]
fn an_event_emitter_method_keeps_its_receiver() {
    let conn = BOUND_CLI
        .replace(
            "(globalThis as any)[String(process.argv.length)](\"err\",",
            "con.on(\"err\",",
        )
        .replace(
            "delete this.own[String(msg)]",
            "delete this.connection.named[String(msg)]",
        );
    assert!(conn.contains("con.on(\"err\", this.onErr.bind(this))"));
    let tail = format!("{MAKE_CALLED}\nimport {{ Cli }} from \"./cli\";\nconst c = new Cli();\nconsole.log(c.own);");
    for interp in [false, true] {
        assert!(
            !imports_formatter_with_mods("", &tail, interp, &[("cli", conn.as_str())]),
            "interp={interp}: an EventEmitter method call keeps the formatter import live"
        );
    }
    // The fixture can fail: an own key unknown code may have written
    // shadows `on`, whose call escapes the receiver again.
    let shadowed = conn.replace(
        "const con = this.connection;",
        "const con = this.connection; (con as any)[String(process.argv.length)] = (globalThis as any)[String(process.argv.length)];",
    );
    assert!(imports_formatter_with_mods("", &tail, false, &[("cli", shadowed.as_str())]));
}

/// Rule 11, soundness: `emit` through the chain calls the listeners `on`
/// stored, with the emit arguments: an any-key delete on the argument a
/// listener receives is one on ⊤ when ⊤ is emitted.
#[test]
fn an_event_emitter_emit_runs_the_stored_listeners() {
    let src = r#"
import { EventEmitter } from "events";
export class Conn extends EventEmitter { named: any = {}; }
export function go() {
  const con = new Conn();
  con.on("x", (o: any) => { delete o[String(o)]; });
  con.emit("x", (globalThis as any)[String(process.argv.length)]);
}
"#;
    let tail = format!("{MAKE_CALLED}\nimport {{ go }} from \"./cli\";\ngo();");
    assert!(imports_formatter_with_mods("", &tail, false, &[("cli", src)]));
    // Without the emit, the listener never runs.
    let quiet = src.replace("con.emit(", "void (");
    assert!(!imports_formatter_with_mods("", &tail, false, &[("cli", quiet.as_str())]));
}

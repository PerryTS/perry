//! Source preparation ahead of the walk must not change what the walk
//! produces: the same program collected with workers and on the walk thread
//! alone yields identical modules and context effects; and the compile's
//! second walk collects the same with the first walk's prepared sources as
//! without them.

use super::source_prep::{PrepClass, PreparedSources, ReadSource, WalkSources};
use super::{collect_modules, collect_modules_with_sources};
use crate::commands::compile::CompilationContext;
use crate::commands::progress::VerboseProgress;
use crate::OutputFormat;
use std::collections::HashSet;
use std::path::Path;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A CommonJS program with a require cycle, an ESM module, a TypeScript
/// class, a JSON module, a text asset, a stdlib require, a computed require
/// (a diagnostic), a dependency shared by several importers and a module the
/// walk prepares after the `import.meta.require` rewrite.
fn fixture(root: &Path) {
    write(
        root,
        "main.js",
        r#"const a = require('./a');
const b = require('./lib/b.js');
const data = require('./data.json');
const os = require('os');
const name = 'c';
const dynamic = require('./' + name);
module.exports = { run() { return a.x + b.y + data.z + dynamic.c(); } };
console.log(module.exports.run(), os.platform().length > 0);
"#,
    );
    write(
        root,
        "a.js",
        r#"const c = require('./c');
exports.x = 1;
exports.later = function () { return c.c(); };
"#,
    );
    write(
        root,
        "c.js",
        r#"const a = require('./a');
exports.c = () => a.x + 1;
"#,
    );
    write(
        root,
        "lib/b.js",
        r#"import { Point } from "./point.ts";
import shared from "./shared.js";
import note from "./note.txt";
import { where } from "./meta.js";
export const y = new Point(2, 0).x + shared.length + note.length + where().length;
"#,
    );
    write(
        root,
        "lib/point.ts",
        r#"import shared from "./shared.js";
export class Point {
  constructor(public x: number, public y: number) {}
  label(): string { return shared + this.x; }
}
"#,
    );
    write(root, "lib/shared.js", "module.exports = 'shared';\n");
    write(
        root,
        "lib/meta.js",
        "export function where() { return typeof require + import.meta.url; }\n",
    );
    write(root, "lib/note.txt", "a text asset\n");
    write(root, "data.json", r#"{ "z": 3 }"#);
}

/// Everything the walk leaves behind that later stages read, as the sorted
/// lines of its `Debug` rendering: HIR types hold `HashMap`s whose iteration
/// order differs between two serial walks, so blocks may move but no line may
/// change, appear or disappear.
fn collect(root: &Path, workers: usize) -> Vec<String> {
    let mut lines: Vec<String> = render(root, workers).lines().map(str::to_string).collect();
    lines.sort();
    lines
}

fn fixture_ctx(root: &Path, workers: usize) -> CompilationContext {
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.entry_canonical = Some(root.join("main.js").canonicalize().unwrap());
    ctx.debug_symbols = true;
    ctx.source_prep_workers = Some(workers);
    ctx
}

fn render(root: &Path, workers: usize) -> String {
    let entry = root.join("main.js");
    let mut ctx = fixture_ctx(root, workers);
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);
    collect_modules(
        &entry,
        &mut ctx,
        &mut visited,
        OutputFormat::Text,
        None,
        &mut next_class_id,
        false,
        &progress,
        None,
    )
    .expect("fixture collects");
    render_ctx(&ctx, visited, next_class_id)
}

fn render_ctx(
    ctx: &CompilationContext,
    visited: HashSet<std::path::PathBuf>,
    next_class_id: perry_hir::ClassId,
) -> String {
    let mut visited: Vec<_> = visited.into_iter().collect();
    visited.sort();
    let mut debug_sources: Vec<_> = ctx
        .cjs_wrap_debug_sources
        .iter()
        .map(|(path, source)| {
            (
                path.clone(),
                source.wrapped_source.clone(),
                source.prefix_line_count,
            )
        })
        .collect();
    debug_sources.sort();
    format!(
        "modules: {:#?}\nvisited: {visited:?}\nnext_class_id: {next_class_id}\n\
         needs_stdlib: {}\nnative_module_imports: {:?}\njs_modules: {:?}\n\
         debug_sources: {debug_sources:?}\nembedded_assets: {:?}",
        ctx.native_modules,
        ctx.needs_stdlib,
        ctx.native_module_imports,
        ctx.js_modules.keys().collect::<Vec<_>>(),
        ctx.embedded_assets,
    )
}

#[test]
fn prepared_ahead_and_serial_walks_collect_identical_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let serial = collect(dir.path(), 0);
    let text = serial.join("\n");
    // The fixture went through the CommonJS wrap and the TypeScript lowering.
    assert!(text.contains("main.js") && text.contains("point.ts"));
    assert!(text.contains("__perry_cjs_factory"));
    assert_eq!(collect(dir.path(), 0), serial, "two serial walks agree");
    // Repeat to give worker timing a chance to vary.
    for _ in 0..4 {
        assert_eq!(collect(dir.path(), 4), serial);
    }
    assert_eq!(collect(dir.path(), 1), serial);
}

/// The fixture walked twice the way a compile walks it
/// (`rerun_collect_with_class_field_types`: the second walk lowers against
/// the first walk's class field types, with computed-require diagnostics
/// off), the second walk reusing the first walk's prepared sources or not.
/// Returns the sorted rendering after the second walk, how many sources the
/// first walk kept and how many the second left unused.
fn collect_twice(root: &Path, workers: usize, reuse: bool) -> (Vec<String>, usize, usize) {
    let entry = root.join("main.js");
    let mut ctx = fixture_ctx(root, workers);
    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1;
    let progress = VerboseProgress::new(OutputFormat::Text, 0);
    let mut prepared = PreparedSources::default();
    let walk = |ctx: &mut CompilationContext,
                visited: &mut HashSet<std::path::PathBuf>,
                next_class_id: &mut perry_hir::ClassId,
                sources: WalkSources| {
        collect_modules_with_sources(
            &entry,
            ctx,
            visited,
            OutputFormat::Text,
            None,
            next_class_id,
            false,
            &progress,
            None,
            sources,
        )
        .expect("fixture collects");
    };
    walk(
        &mut ctx,
        &mut visited,
        &mut next_class_id,
        WalkSources::Keep(&mut prepared),
    );
    let kept = prepared.len();
    let mut field_types = std::collections::HashMap::new();
    for module in ctx.native_modules.values() {
        for class in &module.classes {
            let fields = class
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            field_types.entry(class.name.clone()).or_insert(fields);
        }
    }
    ctx.cross_module_class_field_types = field_types;
    ctx.cjs_require_diagnostics = false;
    ctx.native_modules.clear();
    ctx.reexport_pruner = Default::default();
    visited.clear();
    next_class_id = 1;
    if reuse {
        walk(
            &mut ctx,
            &mut visited,
            &mut next_class_id,
            WalkSources::Reuse(&mut prepared),
        );
    } else {
        // Keeping into a set nobody reads prepares every module afresh.
        walk(
            &mut ctx,
            &mut visited,
            &mut next_class_id,
            WalkSources::Keep(&mut PreparedSources::default()),
        );
    }
    let left = prepared.len();
    let mut lines: Vec<String> = render_ctx(&ctx, visited, next_class_id)
        .lines()
        .map(str::to_string)
        .collect();
    lines.sort();
    (lines, kept, left)
}

#[test]
fn second_walk_with_first_walk_sources_collects_identical_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let (fresh, kept, _) = collect_twice(dir.path(), 0, false);
    // main, a, c, b, point, shared, meta, data.json, note.txt, and the
    // CommonJS support module the wrapped ones import.
    assert_eq!(kept, 10, "the first walk keeps every plain module source");
    assert!(fresh.join("\n").contains("__perry_cjs_factory"));
    for workers in [0, 4, 1] {
        let (reused, kept, left) = collect_twice(dir.path(), workers, true);
        assert_eq!(kept, 10);
        assert_eq!(left, 0, "the second walk takes every kept source");
        assert_eq!(reused, fresh, "workers {workers}");
    }
}

#[test]
fn kept_source_is_reused_only_for_the_same_classification() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let ctx = fixture_ctx(dir.path(), 0);
    let class = PrepClass {
        wrap_eligible: true,
        is_json: false,
        is_text: false,
        is_entry: false,
    };
    let path = dir.path().join("a.js").canonicalize().unwrap();
    let read = super::source_prep::prepare_plain(
        &path,
        class,
        &super::source_prep::PrepInputs::of(&ctx, None),
    )
    .unwrap();
    let mut prepared = PreparedSources::default();
    WalkSources::Keep(&mut prepared).start(&ctx, None);
    prepared.keep(&path, class, &read);
    assert_eq!(prepared.len(), 1);
    // A walk under other settings drops what was kept.
    let mut other = fixture_ctx(dir.path(), 0);
    other.debug_symbols = false;
    WalkSources::Reuse(&mut prepared).start(&other, None);
    assert_eq!(prepared.len(), 0);
    // Same settings, other classification: taken and not used.
    WalkSources::Keep(&mut prepared).start(&ctx, None);
    prepared.keep(&path, class, &read);
    WalkSources::Reuse(&mut prepared).start(&ctx, None);
    let unwrapped = PrepClass {
        wrap_eligible: false,
        ..class
    };
    assert!(prepared.take(&path, unwrapped).is_none());
    assert_eq!(prepared.len(), 0);
    // Same settings and classification: the kept source itself.
    prepared.keep(&path, class, &read);
    let Some(ReadSource::Prepared {
        prepared: Ok(taken),
        ..
    }) = prepared.take(&path, class)
    else {
        panic!("kept source not taken");
    };
    let ReadSource::Prepared {
        prepared: Ok(original),
        ..
    } = &read
    else {
        panic!("a.js is a plain source");
    };
    assert_eq!(taken.source, original.source);
    assert!(taken.was_cjs_wrapped);
}

#[test]
fn source_prepared_after_the_rewrite_is_reused_only_for_the_same_rewrite_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let ctx = fixture_ctx(dir.path(), 0);
    let class = PrepClass {
        wrap_eligible: true,
        is_json: false,
        is_text: false,
        is_entry: false,
    };
    let path = dir.path().join("lib/meta.js").canonicalize().unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    let inputs = super::source_prep::PrepInputs::of(&ctx, None);
    let prepared =
        super::source_prep::prepare_from_raw(raw.clone(), &path, class, &inputs).unwrap();
    let mut sources = PreparedSources::default();
    WalkSources::Keep(&mut sources).start(&ctx, None);
    sources.keep_after_rewrite(&path, class, raw.clone(), &prepared);
    // Not a source prepared straight from the file: left for the rewrite.
    assert!(sources.take(&path, class).is_none());
    assert_eq!(sources.len(), 1);
    // Another rewrite output: taken and not used.
    assert!(sources
        .take_after_rewrite(&path, class, &format!("{raw}\n"))
        .is_none());
    assert_eq!(sources.len(), 0);
    sources.keep_after_rewrite(&path, class, raw.clone(), &prepared);
    let taken = sources
        .take_after_rewrite(&path, class, &raw)
        .expect("same rewrite output");
    assert_eq!(taken.source, prepared.source);
}

#[test]
fn explicit_commonjs_entry_gets_bindings_without_source_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = CompilationContext::new(dir.path().to_path_buf());
    let class = PrepClass {
        wrap_eligible: true,
        is_json: false,
        is_text: false,
        is_entry: true,
    };
    for (filename, wrapped) in [("entry.cjs", true), ("entry.mjs", false)] {
        let path = dir.path().join(filename);
        let prepared = super::source_prep::prepare_from_raw(
            "console.log(typeof module, typeof require, typeof exports);".to_string(),
            &path,
            class,
            &super::source_prep::PrepInputs::of(&ctx, None),
        )
        .unwrap();
        assert_eq!(prepared.was_cjs_wrapped, wrapped, "{filename}");
    }
}

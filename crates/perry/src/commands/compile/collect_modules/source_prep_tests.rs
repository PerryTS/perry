//! Source preparation ahead of the walk must not change what the walk
//! produces: the same program collected with workers and on the walk thread
//! alone yields identical modules and context effects.

use super::collect_modules;
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
/// (a diagnostic) and a dependency shared by several importers.
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
export const y = new Point(2, 0).x + shared.length + note.length;
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

fn render(root: &Path, workers: usize) -> String {
    let entry = root.join("main.js");
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.entry_canonical = Some(entry.canonicalize().unwrap());
    ctx.debug_symbols = true;
    ctx.source_prep_workers = Some(workers);
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

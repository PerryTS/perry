//! Worker entry candidates named by `new URL("<literal>", import.meta.url)`.
//!
//! A program often builds a worker's URL in one place (a helper module, a
//! default option) and constructs the Worker somewhere else, so the call site
//! alone cannot name the file. In a program that uses worker_threads, every
//! such URL literal that names a script is a worker entry candidate: the
//! driver compiles it into the binary and registers it in the run-time worker
//! entry table.

use crate::ir::{Expr, Module};

/// Script extensions a worker entry can have. A URL literal with any other
/// extension (`.json`, `.wasm`, `.html`, ...) names data, not a worker.
const SCRIPT_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs"];

/// A `new URL("<literal>", <base>)` whose literal names a script and whose
/// base is the module's own URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerUrl {
    /// The literal, as written.
    pub literal: String,
    /// The base as a `file:` URL. Lowering folds `import.meta.url` into the
    /// module's own file URL; `None` when the base is still `import.meta.url`.
    pub base: Option<String>,
}

/// Every `new URL("<literal>", import.meta.url)` in `module` whose literal
/// names a script file, in source order, without duplicates. A static
/// `file:` URL base counts too, since lowering folds `import.meta.url` into
/// one.
pub fn worker_url_literals(module: &Module) -> Vec<WorkerUrl> {
    let mut out: Vec<WorkerUrl> = Vec::new();
    super::for_each_module_expr(module, &mut |expr| {
        let Expr::UrlNew {
            url,
            base: Some(base),
        } = expr
        else {
            return;
        };
        let base = match base.as_ref() {
            Expr::ImportMetaUrl(_) => None,
            Expr::String(base) if base.starts_with("file:") => Some(base.clone()),
            _ => return,
        };
        let Expr::String(literal) = url.as_ref() else {
            return;
        };
        let names_script = SCRIPT_EXTENSIONS
            .iter()
            .any(|extension| literal.ends_with(extension));
        let found = WorkerUrl {
            literal: literal.clone(),
            base,
        };
        if names_script && !out.contains(&found) {
            out.push(found);
        }
    });
    out
}

/// Whether `module` uses worker_threads: it imports the module, names it in a
/// string literal (`process.getBuiltinModule("node:worker_threads")`, a table
/// of builtin names), or constructs a Worker.
pub fn module_uses_worker_threads(module: &Module) -> bool {
    let is_name = |name: &str| name == "worker_threads" || name == "node:worker_threads";
    if module.imports.iter().any(|import| is_name(&import.source)) {
        return true;
    }
    let mut found = false;
    super::for_each_module_expr(module, &mut |expr| match expr {
        Expr::String(text) if is_name(text) => found = true,
        Expr::WorkerNew { .. } => found = true,
        _ => {}
    });
    found
}

#[cfg(test)]
mod tests {
    use super::{module_uses_worker_threads, worker_url_literals};

    fn lower(source: &str) -> crate::ir::Module {
        let module = perry_parser::parse_typescript(source, "main.ts").expect("source parses");
        crate::lower_module(&module, "main", "main.ts").expect("source lowers")
    }

    #[test]
    fn script_url_literals_are_found_anywhere() {
        let module = lower(
            r#"
            export const unpack = () => new URL("./unpack-worker.ts", import.meta.url);
            export function registry() { return new URL("./registry-worker.js", import.meta.url); }
            class Pool { entry = new URL("../pool/link-worker.mjs", import.meta.url); }
            const again = new URL("./unpack-worker.ts", import.meta.url);
            const data = new URL("./table.json", import.meta.url);
            const page = new URL("./index.html", import.meta.url);
            const remote = new URL("./remote.ts", "https://example.com/");
            const name = ["./built", ".ts"].join("");
            const built = new URL(name, import.meta.url);
            "#,
        );
        let found = worker_url_literals(&module);
        let literals: Vec<&str> = found.iter().map(|url| url.literal.as_str()).collect();
        assert_eq!(
            literals,
            vec![
                "./unpack-worker.ts",
                "./registry-worker.js",
                "../pool/link-worker.mjs"
            ],
            "only literal script URLs relative to the module, once each"
        );
        assert!(
            found.iter().all(|url| url
                .base
                .as_deref()
                .map_or(true, |base| base.starts_with("file:"))),
            "the base is the module's own file URL: {found:?}"
        );
    }

    #[test]
    fn worker_threads_use_is_detected() {
        assert!(module_uses_worker_threads(&lower(
            r#"import { Worker } from "node:worker_threads"; console.log(Worker);"#
        )));
        assert!(module_uses_worker_threads(&lower(
            r#"export const names = { workers: "node:worker_threads" };"#
        )));
        assert!(module_uses_worker_threads(&lower(
            r#"const wt = process.getBuiltinModule("worker_threads"); console.log(wt);"#
        )));
        assert!(!module_uses_worker_threads(&lower(
            r#"const tpl = new URL("./template.js", import.meta.url); console.log(tpl.href);"#
        )));
    }
}

//! Lightweight AST summaries, never HIR lowering or code generation. A
//! negative export lookup is valid only after traversing every export-star
//! branch; unknown syntax/resolution keeps the edge. Static effect proofs
//! include dependencies outside the declaring package (and terminate cycles).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use super::super::super::CompilationContext;
use super::super::import_helpers::cached_resolve_import_with_lexical_base;
use super::side_effects::Contracts;

#[derive(Clone, Default)]
struct Summary {
    names: HashSet<String>,
    stars: Vec<PathBuf>,
    dependencies: Vec<PathBuf>,
    unknown_exports: bool,
    unknown_dependencies: bool,
    pure: bool,
}

#[derive(Default)]
pub(super) struct Scanner {
    summaries: HashMap<PathBuf, Summary>,
    droppable: HashMap<PathBuf, bool>,
    exports: HashMap<(PathBuf, Option<String>), bool>,
    contracts: Contracts,
}

impl Scanner {
    pub(super) fn module_is_pure(&mut self, path: &Path, ctx: &mut CompilationContext) -> bool {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        self.contracts
            .is_pure(&canonical)
            .unwrap_or_else(|| self.summary(path, ctx).pure)
    }

    fn summary(&mut self, path: &Path, ctx: &mut CompilationContext) -> Summary {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        if let Some(summary) = self.summaries.get(&canonical) {
            return summary.clone();
        }
        // Omitted source still participates in the build-cache proof: edits
        // can add a requested export or an effectful dependency. Its enclosing
        // package manifests are fingerprinted by config_inputs_for as well.
        ctx.resolve_inputs.insert(canonical.clone());
        let mut result = Summary::default();
        let parsed = std::fs::read_to_string(path).ok().and_then(|source| {
            if super::super::super::cjs_wrap::is_commonjs(&source) {
                return None;
            }
            perry_parser::parse_typescript(&source, &path.to_string_lossy()).ok()
        });
        if let Some(module) = parsed {
            let defined = ctx.parsed_defines.apply(&module);
            let module = defined.as_ref().unwrap_or(&module);
            result.pure = super::purity::module_is_pure(module);
            let mut opaque = OpaqueLoads::default();
            module.visit_with(&mut opaque);
            result.unknown_dependencies = opaque.0;
            for item in &module.body {
                let ast::ModuleItem::ModuleDecl(decl) = item else {
                    continue;
                };
                let mut dependency = None;
                let mut star = false;
                match decl {
                    ast::ModuleDecl::Import(import) if !import.type_only => {
                        result.unknown_dependencies |=
                            import.with.is_some() || import.phase != ast::ImportPhase::Evaluation;
                        if import.specifiers.is_empty()
                            || import.specifiers.iter().any(
                                |s| !matches!(s, ast::ImportSpecifier::Named(n) if n.is_type_only),
                            )
                        {
                            dependency = Some(&import.src);
                        }
                    }
                    ast::ModuleDecl::ExportAll(export) if !export.type_only => {
                        result.unknown_dependencies |= export.with.is_some();
                        dependency = Some(&export.src);
                        star = true;
                    }
                    ast::ModuleDecl::ExportNamed(export) if !export.type_only => {
                        result.unknown_dependencies |= export.with.is_some();
                        let mut runtime = export.specifiers.is_empty();
                        for spec in &export.specifiers {
                            let name = match spec {
                                ast::ExportSpecifier::Named(n) if !n.is_type_only => {
                                    Some(export_name(n.exported.as_ref().unwrap_or(&n.orig)))
                                }
                                ast::ExportSpecifier::Namespace(n) => Some(export_name(&n.name)),
                                ast::ExportSpecifier::Default(_) => {
                                    result.unknown_exports = true;
                                    None
                                }
                                _ => None,
                            };
                            if let Some(name) = name {
                                result.names.insert(name);
                                runtime = true;
                            }
                        }
                        if runtime {
                            dependency = export.src.as_ref();
                        }
                    }
                    ast::ModuleDecl::ExportDecl(export) => match &export.decl {
                        ast::Decl::Fn(f) => {
                            result.names.insert(f.ident.sym.to_string());
                        }
                        ast::Decl::Class(c) => {
                            result.names.insert(c.ident.sym.to_string());
                        }
                        ast::Decl::Var(v) => {
                            for decl in &v.decls {
                                pattern_names(&decl.name, &mut result.names);
                            }
                        }
                        ast::Decl::TsEnum(e) => {
                            result.names.insert(e.id.sym.to_string());
                        }
                        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => {}
                        _ => result.unknown_exports = true,
                    },
                    ast::ModuleDecl::ExportDefaultDecl(_)
                    | ast::ModuleDecl::ExportDefaultExpr(_) => {
                        result.names.insert("default".into());
                    }
                    ast::ModuleDecl::TsImportEquals(_) | ast::ModuleDecl::TsExportAssignment(_) => {
                        result.unknown_exports = true;
                        result.unknown_dependencies = true;
                    }
                    _ => {}
                }
                if let Some(source) = dependency {
                    let source = source.value.to_string_lossy();
                    let source = if matches!(decl, ast::ModuleDecl::Import(_)) {
                        ctx.package_aliases
                            .get(source.as_ref())
                            .cloned()
                            .unwrap_or_else(|| source.into_owned())
                    } else {
                        source.into_owned()
                    };
                    if let Some(resolved) =
                        cached_resolve_import_with_lexical_base(&source, path, &canonical, ctx)
                    {
                        if resolved.kind == perry_hir::ModuleKind::NativeRust {
                            result.unknown_dependencies = true;
                            result.unknown_exports |= star;
                        } else if !super::super::super::is_declaration_file(
                            &resolved.canonical_path,
                        ) {
                            result.dependencies.push(resolved.source_path.clone());
                            if star {
                                result.stars.push(resolved.source_path);
                            }
                        }
                    } else {
                        result.unknown_dependencies = true;
                        result.unknown_exports |= star;
                    }
                }
            }
        } else {
            result.unknown_exports = true;
            result.unknown_dependencies = true;
        }
        self.summaries.insert(canonical, result.clone());
        result
    }

    pub(super) fn might_export(
        &mut self,
        path: &Path,
        name: &str,
        ctx: &mut CompilationContext,
    ) -> bool {
        self.exports_match(path, Some(name), ctx)
    }

    pub(super) fn may_have_star_exports(
        &mut self,
        path: &Path,
        ctx: &mut CompilationContext,
    ) -> bool {
        self.exports_match(path, None, ctx)
    }

    fn exports_match(
        &mut self,
        path: &Path,
        name: Option<&str>,
        ctx: &mut CompilationContext,
    ) -> bool {
        let key = (path.to_owned(), name.map(str::to_owned));
        if let Some(result) = self.exports.get(&key) {
            return *result;
        }
        let mut seen = HashSet::new();
        let mut work = vec![path.to_owned()];
        let mut found = false;
        while let Some(path) = work.pop() {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(canonical) {
                continue;
            }
            let summary = self.summary(&path, ctx);
            let matches = match name {
                Some(name) => summary.names.contains(name),
                None => summary.names.iter().any(|name| name != "default"),
            };
            if summary.unknown_exports || matches {
                found = true;
                break;
            }
            work.extend(summary.stars);
        }
        self.exports.insert(key, found);
        found
    }

    pub(super) fn can_drop_tree(&mut self, path: &Path, ctx: &mut CompilationContext) -> bool {
        let root = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        if let Some(result) = self.droppable.get(&root) {
            return *result;
        }
        let mut seen = HashSet::new();
        let mut visiting = HashSet::new();
        let mut work = vec![(path.to_owned(), false)];
        while let Some((path, finished)) = work.pop() {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if finished {
                visiting.remove(&canonical);
                seen.insert(canonical);
                continue;
            }
            if seen.contains(&canonical) {
                continue;
            }
            match self.droppable.get(&canonical) {
                Some(true) => continue,
                Some(false) => {
                    self.droppable.insert(root, false);
                    return false;
                }
                None => {}
            }
            if !visiting.insert(canonical.clone()) {
                // Dropping a barrel edge can change the entry into a retained
                // cycle and thus the values of exported `var` initializers.
                // Package contracts permit omission, not cyclic reordering.
                for ancestor in visiting {
                    self.droppable.insert(ancestor, false);
                }
                self.droppable.insert(root, false);
                return false;
            }
            // Preserve the source-visible base while inference fills the AST
            // summary, just as ordinary collection resolves relative imports.
            if !self.module_is_pure(&path, ctx) {
                self.droppable.insert(root, false);
                return false;
            }
            let summary = self.summary(&path, ctx);
            if summary.unknown_dependencies {
                self.droppable.insert(root, false);
                return false;
            }
            work.push((path, true));
            work.extend(summary.dependencies.into_iter().map(|path| (path, false)));
        }
        // Only cache success for the whole explored set after checking all
        // branches. Caching a partially visited cycle could hide an effect.
        for path in seen {
            self.droppable.insert(path, true);
        }
        true
    }

    pub(super) fn static_tree(&self, root: &Path, seen: &mut HashSet<PathBuf>) {
        let mut work = vec![root.to_owned()];
        while let Some(path) = work.pop() {
            let canonical = path.canonicalize().unwrap_or(path);
            if !seen.insert(canonical.clone()) {
                continue;
            }
            if let Some(summary) = self.summaries.get(&canonical) {
                work.extend(summary.dependencies.iter().cloned());
            }
        }
    }
}

fn export_name(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::Ident(i) => i.sym.to_string(),
        ast::ModuleExportName::Str(s) => s.value.to_string_lossy().into_owned(),
    }
}

fn pattern_names(pattern: &ast::Pat, names: &mut HashSet<String>) {
    match pattern {
        ast::Pat::Ident(i) => {
            names.insert(i.id.sym.to_string());
        }
        ast::Pat::Array(a) => {
            for p in a.elems.iter().flatten() {
                pattern_names(p, names);
            }
        }
        ast::Pat::Object(o) => {
            for p in &o.props {
                match p {
                    ast::ObjectPatProp::KeyValue(p) => pattern_names(&p.value, names),
                    ast::ObjectPatProp::Assign(p) => {
                        names.insert(p.key.id.sym.to_string());
                    }
                    ast::ObjectPatProp::Rest(p) => pattern_names(&p.arg, names),
                }
            }
        }
        ast::Pat::Assign(p) => pattern_names(&p.left, names),
        ast::Pat::Rest(p) => pattern_names(&p.arg, names),
        _ => {}
    }
}

#[derive(Default)]
struct OpaqueLoads(bool);

impl Visit for OpaqueLoads {
    fn visit_ident(&mut self, ident: &ast::Ident) {
        // Aliased require/eval and CommonJS mixed with ESM cannot supply a
        // complete static effect graph. Over-approximating these names is safe.
        if matches!(ident.sym.as_ref(), "require" | "eval" | "module") {
            self.0 = true;
        }
    }

    fn visit_jsx_element(&mut self, _: &ast::JSXElement) {
        self.0 = true;
    }
    fn visit_jsx_fragment(&mut self, _: &ast::JSXFragment) {
        self.0 = true;
    }
}

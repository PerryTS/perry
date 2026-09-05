//! Static recovery for Bun's `import.meta.require` Node-API addon loads.
//!
//! Bun standalone extraction commonly aliases the function and computes an
//! absolute path through `new URL("./addon.node", import.meta.url).pathname`.
//! Neither shape is visible to the CommonJS `require()` scanner. This pass
//! follows immutable local aliases/path constants, authorizes the resolved
//! binary, and replaces the load with `process.dlopen` against the sidecar's
//! portable logical id.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use swc_common::{Globals, Mark, SyntaxContext, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_ast::Pass;
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::{Visit, VisitWith};

use super::native_addon::collect_node_addon_request;
use super::static_require_transform::resolve_static_require;
use super::CompilationContext;

fn strip_transparent_expr(mut expression: &ast::Expr) -> &ast::Expr {
    loop {
        expression = match expression {
            ast::Expr::Paren(value) => &value.expr,
            ast::Expr::TsAs(value) => &value.expr,
            ast::Expr::TsNonNull(value) => &value.expr,
            ast::Expr::TsTypeAssertion(value) => &value.expr,
            ast::Expr::TsConstAssertion(value) => &value.expr,
            ast::Expr::TsSatisfies(value) => &value.expr,
            ast::Expr::TsInstantiation(value) => &value.expr,
            _ => return expression,
        };
    }
}

fn static_member_name(member: &ast::MemberExpr) -> Option<&str> {
    match &member.prop {
        ast::MemberProp::Ident(name) => Some(name.sym.as_ref()),
        ast::MemberProp::Computed(name) => match strip_transparent_expr(&name.expr) {
            ast::Expr::Lit(ast::Lit::Str(value)) => value.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

fn is_import_meta_member(expression: &ast::Expr, expected: &str) -> bool {
    let ast::Expr::Member(member) = strip_transparent_expr(expression) else {
        return false;
    };
    static_member_name(member) == Some(expected)
        && matches!(
            strip_transparent_expr(&member.obj),
            ast::Expr::MetaProp(meta) if meta.kind == ast::MetaPropKind::ImportMeta
        )
}

fn identifier_id(expression: &ast::Expr) -> Option<ast::Id> {
    match strip_transparent_expr(expression) {
        ast::Expr::Ident(identifier) => Some(identifier.to_id()),
        _ => None,
    }
}

fn url_pathname_specifier(
    expression: &ast::Expr,
    unresolved_ctxt: SyntaxContext,
) -> Option<String> {
    let ast::Expr::Member(pathname) = strip_transparent_expr(expression) else {
        return None;
    };
    if static_member_name(pathname) != Some("pathname") {
        return None;
    }
    let ast::Expr::New(url) = strip_transparent_expr(&pathname.obj) else {
        return None;
    };
    let ast::Expr::Ident(constructor) = strip_transparent_expr(&url.callee) else {
        return None;
    };
    if constructor.sym.as_ref() != "URL" || constructor.ctxt != unresolved_ctxt {
        return None;
    }
    let arguments = url.args.as_ref()?;
    if arguments.len() != 2 || arguments.iter().any(|argument| argument.spread.is_some()) {
        return None;
    }
    if !is_import_meta_member(&arguments[1].expr, "url") {
        return None;
    }
    let ast::Expr::Lit(ast::Lit::Str(specifier)) = strip_transparent_expr(&arguments[0].expr)
    else {
        return None;
    };
    specifier.value.as_str().map(str::to_string)
}

fn static_specifier(
    expression: &ast::Expr,
    path_bindings: &HashMap<ast::Id, String>,
    unresolved_ctxt: SyntaxContext,
) -> Option<String> {
    match strip_transparent_expr(expression) {
        ast::Expr::Lit(ast::Lit::Str(value)) => value.value.as_str().map(str::to_string),
        ast::Expr::Ident(identifier) => path_bindings.get(&identifier.to_id()).cloned(),
        expression => url_pathname_specifier(expression, unresolved_ctxt),
    }
}

#[derive(Default)]
struct BindingScan {
    counts: HashMap<ast::Id, usize>,
    const_initializers: HashMap<ast::Id, Vec<Box<ast::Expr>>>,
    modified: HashSet<ast::Id>,
}

impl Visit for BindingScan {
    fn visit_binding_ident(&mut self, binding: &ast::BindingIdent) {
        *self.counts.entry(binding.id.to_id()).or_default() += 1;
        binding.visit_children_with(self);
    }

    fn visit_var_decl(&mut self, declaration: &ast::VarDecl) {
        // Only `const` bindings are forwarding facts. `let`/`var` require a
        // complete mutation analysis (destructuring and loop heads included),
        // while Bun's emitted aliases and path constants use `const`.
        if declaration.kind == ast::VarDeclKind::Const {
            for declarator in &declaration.decls {
                if let (ast::Pat::Ident(binding), Some(initializer)) =
                    (&declarator.name, &declarator.init)
                {
                    self.const_initializers
                        .entry(binding.id.to_id())
                        .or_default()
                        .push(initializer.clone());
                }
            }
        }
        declaration.visit_children_with(self);
    }

    fn visit_assign_expr(&mut self, assignment: &ast::AssignExpr) {
        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(binding)) = &assignment.left
        {
            self.modified.insert(binding.id.to_id());
        }
        assignment.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, update: &ast::UpdateExpr) {
        if let ast::Expr::Ident(identifier) = strip_transparent_expr(&update.arg) {
            self.modified.insert(identifier.to_id());
        }
        update.visit_children_with(self);
    }
}

fn immutable_initializers(scan: &BindingScan) -> HashMap<ast::Id, &ast::Expr> {
    scan.const_initializers
        .iter()
        .filter_map(|(name, initializers)| {
            (scan.counts.get(name) == Some(&1)
                && initializers.len() == 1
                && !scan.modified.contains(name))
            .then(|| (name.clone(), initializers[0].as_ref()))
        })
        .collect()
}

fn recover_require_aliases(initializers: &HashMap<ast::Id, &ast::Expr>) -> HashSet<ast::Id> {
    let mut aliases = HashSet::new();
    loop {
        let mut changed = false;
        for (name, initializer) in initializers {
            if aliases.contains(name) {
                continue;
            }
            let is_alias = is_import_meta_member(initializer, "require")
                || identifier_id(initializer).is_some_and(|source| aliases.contains(&source));
            if is_alias {
                changed |= aliases.insert(name.clone());
            }
        }
        if !changed {
            return aliases;
        }
    }
}

fn recover_path_bindings(
    initializers: &HashMap<ast::Id, &ast::Expr>,
    unresolved_ctxt: SyntaxContext,
) -> HashMap<ast::Id, String> {
    let mut paths = HashMap::new();
    loop {
        let mut changed = false;
        for (name, initializer) in initializers {
            if paths.contains_key(name) {
                continue;
            }
            if let Some(specifier) = static_specifier(initializer, &paths, unresolved_ctxt) {
                paths.insert(name.clone(), specifier);
                changed = true;
            }
        }
        if !changed {
            return paths;
        }
    }
}

struct RequireCall {
    start: usize,
    end: usize,
    specifier: Option<String>,
    direct: bool,
}

struct CallScan<'a> {
    aliases: &'a HashSet<ast::Id>,
    paths: &'a HashMap<ast::Id, String>,
    unresolved_ctxt: SyntaxContext,
    calls: Vec<RequireCall>,
}

impl Visit for CallScan<'_> {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        let direct = matches!(&call.callee, ast::Callee::Expr(callee)
            if is_import_meta_member(callee, "require"));
        let recognized = match &call.callee {
            ast::Callee::Expr(callee) => {
                direct || identifier_id(callee).is_some_and(|id| self.aliases.contains(&id))
            }
            _ => false,
        };
        if recognized {
            let specifier = (call.args.len() == 1 && call.args[0].spread.is_none())
                .then(|| static_specifier(&call.args[0].expr, self.paths, self.unresolved_ctxt))
                .flatten();
            self.calls.push(RequireCall {
                start: call.span.lo.0.saturating_sub(1) as usize,
                end: call.span.hi.0.saturating_sub(1) as usize,
                specifier,
                direct,
            });
        }
        call.visit_children_with(self);
    }
}

fn looks_like_node_addon(specifier: &str) -> bool {
    specifier
        .split(['?', '#'])
        .next()
        .is_some_and(|path| path.ends_with(".node"))
}

fn unique_identifier(source: &str, prefix: &str, index: usize) -> String {
    let mut suffix = index;
    loop {
        let name = format!("{prefix}_{suffix}");
        if !source.contains(&name) {
            return name;
        }
        suffix += 1;
    }
}

pub(super) fn rewrite_import_meta_require_addons(
    source: &str,
    module_path: &Path,
    ctx: &mut CompilationContext,
) -> Result<String> {
    if !source.contains("import.meta") || !source.contains("require") {
        return Ok(source.to_string());
    }
    let filename = module_path.to_string_lossy();
    // A `.js` file that only uses `import.meta` has no import/export token for
    // the parser's script-vs-module heuristic. Append a zero-width-for-existing
    // spans module marker for this analysis parse; the emitted source and all
    // original byte offsets remain unchanged.
    let analysis_source = format!("{source}\nexport {{}};\n");
    let module = perry_parser::parse_typescript(&analysis_source, &filename).map_err(|error| {
        anyhow!(
            "failed to analyze `import.meta.require` in {}: {error}",
            module_path.display()
        )
    })?;
    // SWC's resolver assigns a distinct syntax context to every lexical
    // binding and its references. That makes the following dataflow safe
    // across nested scopes: a parameter named `load` cannot be mistaken for
    // an outer `const load = import.meta.require` alias.
    let mut program = ast::Program::Module(module);
    let unresolved_ctxt = GLOBALS.set(&Globals::new(), || {
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();
        resolver(unresolved_mark, top_level_mark, true).process(&mut program);
        SyntaxContext::empty().apply_mark(unresolved_mark)
    });
    let ast::Program::Module(module) = program else {
        unreachable!("the resolver preserves a module program")
    };
    let mut bindings = BindingScan::default();
    module.visit_with(&mut bindings);
    let initializers = immutable_initializers(&bindings);
    let aliases = recover_require_aliases(&initializers);
    let paths = recover_path_bindings(&initializers, unresolved_ctxt);
    let mut calls = CallScan {
        aliases: &aliases,
        paths: &paths,
        unresolved_ctxt,
        calls: Vec::new(),
    };
    module.visit_with(&mut calls);

    let mut replacements = Vec::new();
    let process_alias = unique_identifier(source, "__perry_import_meta_process", 0);
    for (index, call) in calls.calls.into_iter().enumerate() {
        let Some(specifier) = call.specifier else {
            // #9742: direct calls can load ordinary JS chunks. Let the HIR
            // synchronous-require resolver handle their bounded candidate set
            // and runtime fallback; an unknown path is not proof of an addon.
            // Aliased addon loading keeps its existing declaration diagnostic.
            if call.direct {
                continue;
            }
            anyhow::bail!(
                "cannot statically prove the Node-API addon path passed to `import.meta.require` in {}. Declare every project-owned addon with an exact `perry.nativeAddonPaths` entry and call the unmodified binding with a string literal or `new URL(\"./addon.node\", import.meta.url).pathname`.",
                module_path.display()
            );
        };
        let target = resolve_static_require(
            module_path.parent().unwrap_or_else(|| Path::new(".")),
            &specifier,
            ctx.bunfs_root.as_deref(),
        );
        let Some(target) = target else {
            if looks_like_node_addon(&specifier) {
                anyhow::bail!(
                    "statically declared Node-API addon `{specifier}` from {} could not be resolved. Project-owned addons must be listed by exact path in `perry.nativeAddonPaths`.",
                    module_path.display()
                );
            }
            continue;
        };
        if target.extension().and_then(|extension| extension.to_str()) != Some("node") {
            continue;
        }
        let Some(logical_id) = collect_node_addon_request(ctx, &target)? else {
            continue;
        };
        if call.start > call.end
            || call.end > source.len()
            || !source.is_char_boundary(call.start)
            || !source.is_char_boundary(call.end)
        {
            anyhow::bail!(
                "invalid source span while rewriting `import.meta.require` in {}",
                module_path.display()
            );
        }
        let temporary = unique_identifier(source, "__perry_import_meta_addon", index);
        let request = serde_json::to_string(&logical_id)?;
        let replacement = format!(
            "(function() {{ const {temporary} = {{ exports: {{}} }}; {process_alias}.dlopen({temporary}, {request}); return {temporary}.exports; }})()"
        );
        replacements.push((call.start, call.end, replacement));
    }

    replacements.sort_by_key(|(start, _, _)| *start);
    let mut rewritten = source.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(start..end, &replacement);
    }
    if rewritten != source {
        let import = format!("import * as {process_alias} from \"node:process\";\n");
        if rewritten.starts_with("#!") {
            if let Some(line_end) = rewritten.find('\n') {
                rewritten.insert_str(line_end + 1, &import);
            } else {
                rewritten.push('\n');
                rewritten.push_str(&import);
            }
        } else {
            rewritten.insert_str(0, &import);
        }
    }
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> (tempfile::TempDir, PathBuf, CompilationContext) {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("native");
        std::fs::create_dir_all(&native).unwrap();
        let addon = native.join("addon.node");
        std::fs::copy(std::env::current_exe().unwrap(), &addon).unwrap();
        let addon = addon.canonicalize().unwrap();
        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.bunfs_root = Some(dir.path().canonicalize().unwrap());
        ctx.native_addon_paths
            .insert(addon, "native/addon.node".to_string());
        (dir, PathBuf::from("main.js"), ctx)
    }

    #[test]
    fn follows_aliases_url_path_constants_and_bun_virtual_paths() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let source = r#"
const load = import.meta.require;
const forwarded = load;
const addonPath = new URL("./native/addon.node", import.meta.url).pathname;
const a = forwarded(addonPath);
const b = load("./native/addon.node");
const c = load("/$bunfs/root/native/addon.node");
const d = import.meta.require(new URL("./native/addon.node", import.meta.url).pathname);
"#;
        let rewritten = rewrite_import_meta_require_addons(source, &entry, &mut ctx).unwrap();
        assert_eq!(rewritten.matches(".dlopen(").count(), 4, "{rewritten}");
        assert_eq!(
            rewritten.matches("\"$project/native/addon.node\"").count(),
            4
        );
        assert_eq!(ctx.native_addons.len(), 1);
        assert!(!ctx.native_addons["$project/native/addon.node"].ship_package_payload);
    }

    #[test]
    fn rejects_a_dynamic_path_with_policy_guidance() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let error = rewrite_import_meta_require_addons(
            "const load = import.meta.require; load(process.env.ADDON_PATH);",
            &entry,
            &mut ctx,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cannot statically prove"), "{error}");
        assert!(error.contains("perry.nativeAddonPaths"), "{error}");
    }

    #[test]
    fn direct_runtime_paths_reach_synchronous_module_dispatch() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let source = "import.meta.require(process.argv[2]); import.meta['require'](choosePath());";
        assert_eq!(
            rewrite_import_meta_require_addons(source, &entry, &mut ctx).unwrap(),
            source
        );
        assert!(ctx.native_addons.is_empty());
    }

    #[test]
    fn does_not_follow_a_modified_binding() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let source =
            r#"let load = import.meta.require; load = other; load("./native/addon.node");"#;
        let rewritten = rewrite_import_meta_require_addons(source, &entry, &mut ctx).unwrap();
        assert_eq!(rewritten, source);
        assert!(ctx.native_addons.is_empty());
    }

    #[test]
    fn does_not_treat_a_shadowed_url_constructor_as_static() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let error = rewrite_import_meta_require_addons(
            r#"const URL = CustomURL; const load = import.meta.require; load(new URL("./native/addon.node", import.meta.url).pathname);"#,
            &entry,
            &mut ctx,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cannot statically prove"), "{error}");
    }

    #[test]
    fn follows_nested_aliases_without_rewriting_same_named_bindings() {
        let (dir, relative_entry, mut ctx) = fixture();
        let entry = dir.path().join(relative_entry);
        let source = r#"
function loadAddon() {
  const load = import.meta.require;
  return load("./native/addon.node");
}
function unrelated(load) {
  return load("./native/addon.node");
}
"#;
        let rewritten = rewrite_import_meta_require_addons(source, &entry, &mut ctx).unwrap();
        assert_eq!(rewritten.matches(".dlopen(").count(), 1, "{rewritten}");
        assert!(
            rewritten.contains("return load(\"./native/addon.node\");\n}"),
            "{rewritten}"
        );
    }
}

//! A closed-world refusal census over ORIGINAL source. A loader value,
//! module record, global object or dynamic-code capability can expose the
//! CommonJS cache/factory; such programs keep per-evaluation classes.
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};
fn name(m: &ast::MemberExpr) -> Option<String> {
    perry_hir::patched_builtins::static_member_name(&m.prop)
}
fn native_capability(source: &str) -> bool {
    source.ends_with(".node")
        || source.starts_with("data:")
        || matches!(
            source,
            "module"
                | "node:module"
                | "vm"
                | "node:vm"
                | "process"
                | "node:process"
                | "inspector"
                | "node:inspector"
                | "inspector/promises"
                | "node:inspector/promises"
                | "repl"
                | "node:repl"
                | "bun:ffi"
        )
}
fn loader_property(name: &str) -> bool {
    matches!(
        name,
        "cache"
            | "_cache"
            | "require"
            | "mainModule"
            | "getBuiltinModule"
            | "createRequire"
            | "binding"
            | "_linkedBinding"
            | "constructor"
            | "__proto__"
            | "getPrototypeOf"
            | "getOwnPropertyDescriptor"
            | "getOwnPropertyDescriptors"
    )
}
#[derive(Default)]
struct Uses(u32);
impl Visit for Uses {
    fn visit_import_decl(&mut self, i: &ast::ImportDecl) {
        if native_capability(i.src.value.to_string_lossy().as_ref()) {
            self.0 += 1;
        }
        i.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        // A dynamic import or native addon can expose code/capabilities
        // outside the compiled module bodies. Keep evaluations fresh.
        if matches!(c.callee, ast::Callee::Import(_)) {
            self.0 += 1;
        }
        // A proven non-loader builtin does not expose module records. An
        // unknown/computed name still visits the process capability below.
        if let ast::Callee::Expr(callee) = &c.callee {
            if let ast::Expr::Member(m) = callee.as_ref() {
                if matches!(m.obj.as_ref(), ast::Expr::Ident(i) if i.sym == *"process")
                    && name(m).as_deref() == Some("getBuiltinModule")
                {
                    if let [arg] = c.args.as_slice() {
                        if arg.spread.is_none() {
                            if let ast::Expr::Lit(ast::Lit::Str(source)) = arg.expr.as_ref() {
                                if !native_capability(source.value.to_string_lossy().as_ref()) {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        // The only unescaped require value: an ordinary literal call. Its
        // module result cannot expose the loader except the module/vm APIs.
        if matches!(&c.callee, ast::Callee::Expr(e) if matches!(e.as_ref(), ast::Expr::Ident(i) if i.sym == *"require"))
        {
            if let [arg] = c.args.as_slice() {
                if arg.spread.is_none() {
                    if let ast::Expr::Lit(ast::Lit::Str(s)) = arg.expr.as_ref() {
                        if native_capability(s.value.to_string_lossy().as_ref()) {
                            self.0 += 1;
                        }
                        return;
                    }
                }
            }
        }
        c.visit_children_with(self);
    }
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        // Ordinary CJS publication and ordinary argv/timer/output reads do
        // not expose a loader. Accessing their constructor still refuses at
        // the outer member below.
        if matches!(m.obj.as_ref(), ast::Expr::Ident(i) if i.sym == *"module")
            && name(m).as_deref() == Some("exports")
        {
            return;
        }
        // A named `process` property other than a loader capability
        // (`mainModule`, `binding`, `getBuiltinModule`, …) or a native-code
        // load (`dlopen`) cannot reach a module record. A computed key can.
        if matches!(m.obj.as_ref(), ast::Expr::Ident(i) if i.sym == *"process")
            && name(m)
                .as_deref()
                .is_some_and(|n| !loader_property(n) && n != "dlopen")
        {
            return;
        }
        if name(m).as_deref().is_some_and(loader_property) {
            self.0 += 1;
        }
        m.visit_children_with(self);
    }
    fn visit_object_pat_prop(&mut self, p: &ast::ObjectPatProp) {
        // Pattern keys are IdentName, not identifier reads: the ordinary
        // identifier visitor never sees `{createRequire: alias}`.
        let reaches = match p {
            ast::ObjectPatProp::KeyValue(kv) => match &kv.key {
                ast::PropName::Ident(i) => loader_property(i.sym.as_ref()),
                ast::PropName::Str(s) => loader_property(s.value.to_string_lossy().as_ref()),
                ast::PropName::Computed(c) => match c.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(s)) => {
                        loader_property(s.value.to_string_lossy().as_ref())
                    }
                    ast::Expr::Lit(_) => false,
                    _ => true, // An unknown key may select a loader capability.
                },
                _ => false,
            },
            ast::ObjectPatProp::Assign(a) => loader_property(a.key.id.sym.as_ref()),
            ast::ObjectPatProp::Rest(_) => false,
        };
        self.0 += u32::from(reaches);
        p.visit_children_with(self);
    }
    fn visit_ident(&mut self, i: &ast::Ident) {
        if matches!(
            i.sym.as_ref(),
            "require"
                | "module"
                | "process"
                | "globalThis"
                | "global"
                | "window"
                | "self"
                | "eval"
                | "Function"
                | "Reflect"
                | "createRequire"
                | "getBuiltinModule"
                | "mainModule"
                | "Bun"
                | "Deno"
        ) {
            self.0 += 1;
        }
    }
}
pub(super) fn module_capability_uses(module: &ast::Module) -> u32 {
    let mut uses = Uses::default();
    module.visit_with(&mut uses);
    uses.0
}
#[cfg(test)]
mod tests {
    use super::*;
    fn uses(s: &str) -> u32 {
        module_capability_uses(&perry_parser::parse_typescript(s, "proof.cjs").unwrap())
    }
    #[test]
    fn closed_calls_and_cache_or_loader_escape_are_distinct() {
        assert_eq!(uses("const {Base} = require('./base'); class C extends Base {} module.exports = C; console.log(process.argv[2]);"), 0);
        assert_eq!(uses("const fs = require('node:fs'); const path = require('node:path'); process.getBuiltinModule('fs');"), 0);
        assert_eq!(uses("process.exitCode = 1; process.on('exit', f); process.nextTick(g); console.log(process.platform, process.versions.node);"), 0);
        for source in [
            "delete require.cache[require.resolve('./child')];",
            "const r = require; r('./child');",
            "require['cache'];",
            "const {cache} = require;",
            "require('module');",
            "import {createRequire} from 'node:module';",
            "module.parent.require('./child');",
            "process.getBuiltinModule('module');",
            "globalThis.process;",
            "Object.getPrototypeOf(function(){});",
            "({}).__proto__;",
            "require('./addon.node');",
            "import native from './addon.node';",
            "import('./plugin.js');",
            "Bun.dlopen('x');",
            "const key = 'get' + 'BuiltinModule'; process[key]('module');",
            "const key = process.argv[2]; process[key]('module');",
            "const {createRequire: cr} = host; cr('x');",
            "host.createRequire('x');",
            "const {['createRequire']: cr} = host; cr('x');",
            "const {[key]: cr} = host; cr('x');",
            "const {_cache: cache} = host; delete cache[k];",
            "import x from 'data:text/javascript,export default 1';",
            "require('node:repl');",
            "process.dlopen(m, './x.node');",
            "process.binding('fs');",
            "process.mainModule.require('./x');",
            "process[name];",
            "f(process);",
        ] {
            assert!(uses(source) > 0, "{source}");
        }
    }
    /// Design CJS_LINK §2.5: each listed capability makes the program not
    /// closed; a program requiring only builtins is closed.
    #[test]
    fn each_loader_capability_makes_the_program_not_closed() {
        assert_eq!(uses("const fs = require('fs'); const p = require('node:path'); module.exports = fs.existsSync(p.sep);"), 0);
        for source in [
            // loader capabilities
            "require('module');",
            "require('node:module').createRequire(__filename);",
            "import Module from 'node:module'; Module._cache;",
            "import Module from 'module'; Module._load('x');",
            "import Module from 'module'; Module.prototype.require;",
            "require('vm');",
            "require('node:inspector');",
            "require('repl');",
            "process.getBuiltinModule('node:module');",
            "require('./native.node');",
            "import('data:text/javascript,1');",
            "import('./x.js');",
            // cache and extensions writes or escapes
            "require.cache[k] = m;",
            "delete require.cache[k];",
            "require.cache = {};",
            "require.extensions['.txt'] = f;",
            "f(require.cache);",
            "f(require.extensions);",
            // computed require
            "require(name);",
            "require('./' + name);",
            // require or module leaving as a value
            "const r = require; r('./x');",
            "holder.r = require;",
            "f(require);",
            "holder.m = module;",
            "f(module);",
        ] {
            assert!(uses(source) > 0, "{source}");
        }
    }
}

//! Closed allowlist over the original AST, before folding or builtin lowering.
//! Adding an expression requires proving both its value and absence of effects.
use std::collections::HashMap;

use swc_ecma_ast::{Callee, Decl, Expr, Lit, MemberProp, Module, ModuleItem, Pat, Stmt, VarDeclKind};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Output {
    pub fd: u8,
    pub bytes: Vec<u8>,
}

/// Only this module can construct the proof consumed by minimal codegen.
#[derive(Debug)]
pub(super) struct ProvenProgram {
    outputs: Vec<Output>,
}

impl ProvenProgram {
    pub(super) fn outputs(&self) -> &[Output] {
        &self.outputs
    }
}

pub(super) fn analyze(source: &str, filename: &str) -> Result<ProvenProgram, &'static str> {
    if source.len() > MAX_BYTES {
        return Err("source exceeds tiny analysis limit");
    }
    let mut cache = perry_diagnostics::SourceCache::new();
    let parsed = perry_parser::parse_typescript_with_cache(source, filename, &mut cache)
        .map_err(|_| "source did not parse cleanly")?;
    if !parsed.diagnostics.is_empty() {
        return Err("parser recovery requires normal compilation");
    }
    analyze_module(&parsed.module)
}

fn analyze_module(module: &Module) -> Result<ProvenProgram, &'static str> {
    let mut strings = HashMap::<String, String>::new();
    let mut outputs = Vec::new();
    let mut retained_bytes = 0usize;
    for item in &module.body {
        let ModuleItem::Stmt(stmt) = item else {
            return Err("imports and exports require normal compilation");
        };
        match stmt {
            Stmt::Empty(_) => {}
            Stmt::Decl(Decl::Var(var)) if var.kind == VarDeclKind::Const && !var.declare => {
                for decl in &var.decls {
                    let Pat::Ident(binding) = &decl.name else {
                        return Err("destructuring is outside the proven subset");
                    };
                    let name = binding.id.sym.to_string();
                    // Reject even a later lexical declaration: the receiver
                    // would be in its TDZ at an earlier console call.
                    if name == "console" || strings.contains_key(&name) {
                        return Err("shadowed console or duplicate lexical binding");
                    }
                    let expr = decl.init.as_deref().ok_or("uninitialized binding")?;
                    let value = constant_string(expr, &strings, 0)?;
                    charge(&mut retained_bytes, value.len())?;
                    strings.insert(name, value);
                }
            }
            Stmt::Expr(stmt) => {
                if matches!(stmt.expr.as_ref(), Expr::Lit(Lit::Str(_))) {
                    continue; // Directives / effect-free string expression.
                }
                let Expr::Call(call) = stmt.expr.as_ref() else {
                    return Err("expression is outside the proven subset");
                };
                let Callee::Expr(callee) = &call.callee else {
                    return Err("unknown callee");
                };
                let Expr::Member(member) = callee.as_ref() else {
                    return Err("unknown callee");
                };
                if !matches!(member.obj.as_ref(), Expr::Ident(id) if id.sym == *"console") {
                    return Err("console receiver is not the original builtin");
                }
                let MemberProp::Ident(property) = &member.prop else {
                    return Err("computed console access is not proven");
                };
                let fd = match property.sym.as_ref() {
                    "log" => 1,
                    "error" => 2,
                    _ => return Err("console method is outside the proven subset"),
                };
                if call.args.len() != 1 || call.args[0].spread.is_some() {
                    return Err("console requires one constant string argument");
                }
                let mut bytes = constant_string(&call.args[0].expr, &strings, 0)?.into_bytes();
                bytes.push(b'\n');
                charge(&mut retained_bytes, bytes.len())?;
                outputs.push(Output { fd, bytes });
            }
            _ => return Err("statement is outside the proven subset"),
        }
    }
    Ok(ProvenProgram { outputs })
}

fn charge(total: &mut usize, bytes: usize) -> Result<(), &'static str> {
    *total = total.checked_add(bytes).ok_or("constant storage overflow")?;
    if *total > MAX_BYTES {
        return Err("constant storage exceeds tiny analysis limit");
    }
    Ok(())
}

fn constant_string(
    expr: &Expr,
    strings: &HashMap<String, String>,
    depth: usize,
) -> Result<String, &'static str> {
    if depth >= MAX_DEPTH {
        return Err("expression exceeds tiny analysis depth");
    }
    let value = match expr {
        Expr::Lit(Lit::Str(s)) => s.value.to_string_lossy().into_owned(),
        Expr::Ident(id) => strings
            .get(id.sym.as_ref())
            .cloned()
            .ok_or("string binding is not initialized and proven")?,
        Expr::Tpl(template) => {
            let mut text = String::new();
            let mut size = 0;
            for (index, quasi) in template.quasis.iter().enumerate() {
                let part = quasi.cooked.as_ref().ok_or("uncooked template")?.to_string_lossy();
                charge(&mut size, part.len())?;
                text.push_str(&part);
                if let Some(expr) = template.exprs.get(index) {
                    let part = constant_string(expr, strings, depth + 1)?;
                    charge(&mut size, part.len())?;
                    text.push_str(&part);
                }
            }
            text
        }
        Expr::Paren(p) => constant_string(&p.expr, strings, depth + 1)?,
        Expr::TsAs(p) => constant_string(&p.expr, strings, depth + 1)?,
        Expr::TsTypeAssertion(p) => constant_string(&p.expr, strings, depth + 1)?,
        Expr::TsNonNull(p) => constant_string(&p.expr, strings, depth + 1)?,
        Expr::TsSatisfies(p) => constant_string(&p.expr, strings, depth + 1)?,
        _ => return Err("argument is not a proven effect-free string"),
    };
    if value.len() > MAX_BYTES {
        return Err("string exceeds tiny analysis limit");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proves_empty_and_constant_template_output() {
        assert!(analyze("// empty\n;", "app.ts").unwrap().outputs().is_empty());
        let plan = analyze(
            "const who = 'world'; const greeting = `hello, ${who}`; console.log(greeting); console.error('done');",
            "app.ts",
        ).unwrap();
        assert_eq!(plan.outputs(), &[
            Output { fd: 1, bytes: b"hello, world\n".to_vec() },
            Output { fd: 2, bytes: b"done\n".to_vec() },
        ]);
    }

    #[test]
    fn preserves_bytes_and_erased_string_assertions() {
        let plan = analyze(r#"console.log(('a\0%\n🦀\uD800' as string)!);"#, "app.ts").unwrap();
        assert_eq!(plan.outputs()[0].bytes, "a\0%\n🦀�\n".as_bytes());
    }

    #[test]
    fn every_unproven_construct_falls_back() {
        let sources = [
            "let s = 'a'; console.log(s);", "var s = 'a'; console.log(s);",
            "const console = 'fake'; console.log('a');",
            "console.log('a'); const console = 'fake';",
            "console.log('a'); function console() {}",
            "console.log('a'); var console;",
            "const {log} = console; log('a');", "const c = console; c.log('a');",
            "console['log']('a');", "console.log?.('a');", "console?.log('a');",
            "console.log = () => {}; console.log('a');",
            "console.log((console.log = () => {}, 'a'));",
            "console.log({ get x() { return 'a'; } });",
            "console.log('a', 'b');", "console.log(...['a']);", "console.log(42);",
            "console.log(`n=${42}`);", "console.log('a' + 'b');",
            "console.log(s); const s = 'a';", "const s = s;",
            "const s = 'a'; const s = 'b';", "const s = 'a'; s = 'b';",
            "console.log(String('a'));", "globalThis.console.log('a');",
            "import 'node:fs'; console.log('a');", "import type {T} from './t';",
            "export const s = 'a';", "export {};", "function unused() {}",
            "if (false) { new Promise(() => {}); } console.log('a');",
            "while (false) { const x = []; }", "for (;;) { const x = {}; }",
            "class C {}", "const f = () => 'a';", "const x = [];",
            "Promise.resolve('a');", "queueMicrotask(() => {});", "setTimeout(() => {}, 0);",
            "process.on('exit', () => {});", "process.argv;", "eval('');",
            "new Function('return this')();", "import('node:fs');",
            "throw 'a';", "try { console.log('a'); } catch {}",
            "using x = null;", "await Promise.resolve();", "namespace N {}",
            "interface T {}", "declare const x: string;", "debugger;",
        ];
        for source in sources {
            assert!(analyze(source, "app.ts").is_err(), "incorrectly admitted {source}");
        }
    }

    #[test]
    fn escaped_identifiers_do_not_hide_receiver_shadowing() {
        assert!(analyze(r#"const \u0063onsole = 'x'; console.log('a');"#, "app.ts").is_err());
        assert!(analyze(r#"console.log('a'); const \u0063onsole = 'x';"#, "app.ts").is_err());
        assert_eq!(analyze(r#"\u0063onsole.log('a');"#, "app.ts").unwrap().outputs()[0].bytes, b"a\n");
    }

    #[test]
    fn expansion_is_bounded() {
        let mut source = String::from("const s0 = 'abcdefgh';");
        for i in 1..20 {
            source.push_str(&format!("const s{i} = `${{s{}}}${{s{}}}`;", i-1, i-1));
        }
        assert!(analyze(&source, "app.ts").is_err());
    }
}

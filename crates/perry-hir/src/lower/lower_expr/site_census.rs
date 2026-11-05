//! Site census (`PERRY_FACTS_DUMP` only): one `<dump>.census` line per
//! source member read, element read, call, `new`, member write, update and
//! delete, naming the HIR variant its lowering produced. It is the source
//! denominator of the facts ledger: a site the lowering folds, specializes
//! or drops still has a line. Nothing in the compiler reads it back.
//!
//! Line: `path \t start \t end \t kind \t key \t op \t lowered` (SWC 1-based
//! byte span, end exclusive, the coordinates of HIR `byte_offset`).

use super::*;

/// The variant name of `e`: its `Debug` output up to the first non-name
/// character (the writer refuses the rest, so the tree is not printed).
fn variant(e: &Expr) -> String {
    struct Head(String);
    impl std::fmt::Write for Head {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            for c in s.chars() {
                if c.is_alphanumeric() || c == '_' {
                    self.0.push(c);
                } else {
                    return Err(std::fmt::Error);
                }
            }
            Ok(())
        }
    }
    let mut h = Head(String::new());
    let _ = std::fmt::Write::write_fmt(&mut h, format_args!("{e:?}"));
    h.0
}

fn unparen(e: &ast::Expr) -> &ast::Expr {
    match e {
        ast::Expr::Paren(p) => unparen(&p.expr),
        ast::Expr::TsAs(a) => unparen(&a.expr),
        ast::Expr::TsNonNull(n) => unparen(&n.expr),
        ast::Expr::TsTypeAssertion(a) => unparen(&a.expr),
        ast::Expr::TsSatisfies(s) => unparen(&s.expr),
        ast::Expr::TsConstAssertion(c) => unparen(&c.expr),
        _ => e,
    }
}

/// `(element, key)` of a member property.
fn member_key(p: &ast::MemberProp) -> (bool, String) {
    match p {
        ast::MemberProp::Ident(i) => (false, i.sym.to_string()),
        ast::MemberProp::PrivateName(p) => (false, format!("#{}", p.name)),
        ast::MemberProp::Computed(_) => (true, "[]".into()),
    }
}

fn super_key(p: &ast::SuperProp) -> String {
    match p {
        ast::SuperProp::Ident(i) => i.sym.to_string(),
        ast::SuperProp::Computed(_) => "[]".into(),
    }
}

fn callee_key(e: &ast::Expr) -> String {
    match unparen(e) {
        ast::Expr::Member(m) => member_key(&m.prop).1,
        ast::Expr::SuperProp(s) => super_key(&s.prop),
        _ => "()".into(),
    }
}

/// Kind, key and operator of a site expression; `None` for any other.
fn site(e: &ast::Expr) -> Option<(&'static str, String, String)> {
    let plain = |k: &'static str, key: String| Some((k, key, String::new()));
    match e {
        ast::Expr::Member(m) => match member_key(&m.prop) {
            (true, key) => plain("element", key),
            (false, key) => plain("property", key),
        },
        ast::Expr::SuperProp(s) => plain("super-property", super_key(&s.prop)),
        ast::Expr::Call(c) => match &c.callee {
            ast::Callee::Expr(x) => plain("call", callee_key(x)),
            ast::Callee::Super(_) => plain("super-call", "super".into()),
            ast::Callee::Import(_) => plain("import-call", "import".into()),
        },
        ast::Expr::TaggedTpl(t) => plain("tagged-call", callee_key(&t.tag)),
        ast::Expr::New(n) => plain(
            "new",
            match unparen(&n.callee) {
                ast::Expr::Ident(i) => i.sym.to_string(),
                ast::Expr::Member(m) => member_key(&m.prop).1,
                _ => "new".into(),
            },
        ),
        ast::Expr::OptChain(o) => match &*o.base {
            ast::OptChainBase::Member(m) => match member_key(&m.prop) {
                (true, key) => plain("optional-element", key),
                (false, key) => plain("optional-property", key),
            },
            ast::OptChainBase::Call(c) => plain("optional-call", callee_key(&c.callee)),
        },
        ast::Expr::Assign(a) => {
            let (kind, key) = match &a.left {
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) => {
                    match member_key(&m.prop) {
                        (true, key) => ("element-write", key),
                        (false, key) => ("write", key),
                    }
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::SuperProp(s)) => {
                    ("super-write", super_key(&s.prop))
                }
                _ => return None,
            };
            Some((kind, key, a.op.as_str().to_string()))
        }
        ast::Expr::Update(u) => match unparen(&u.arg) {
            ast::Expr::Member(m) => match member_key(&m.prop) {
                (true, key) => Some(("element-update", key, u.op.as_str().to_string())),
                (false, key) => Some(("update", key, u.op.as_str().to_string())),
            },
            _ => None,
        },
        ast::Expr::Unary(u) if u.op == ast::UnaryOp::Delete => match unparen(&u.arg) {
            ast::Expr::Member(m) => plain("delete", member_key(&m.prop).1),
            _ => None,
        },
        _ => None,
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Append the census line of `expr` (lowered to `lowered`) to
/// `<dump>.census`.
pub(super) fn record(ctx: &LoweringContext, expr: &ast::Expr, lowered: &Result<Expr>) {
    let Some((kind, key, op)) = site(expr) else {
        return;
    };
    let Some(dump) = std::env::var_os("PERRY_FACTS_DUMP") else {
        return;
    };
    let span = expr.span();
    let to = match lowered {
        Ok(e) => variant(e),
        Err(_) => "lowering-error".into(),
    };
    let line = format!(
        "{}\t{}\t{}\t{kind}\t{}\t{op}\t{to}\n",
        esc(&ctx.source_file_path),
        span.lo.0,
        span.hi.0,
        esc(&key),
    );
    let mut path = dump;
    path.push(".census");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

//! #1678 (Phase 0 of #1677) — classify `new Function` / `Function(...)` /
//! `eval(...)` call sites and emit a precise refusal diagnostic.
//!
//! Perry is an ahead-of-time compiler: it never executes a code string at
//! runtime. Before this module, the `Function`/`eval` shapes silently fell
//! through to a broken lowering — a bare `Function`/`eval` ident lowers to
//! the `GlobalGet(0)` sentinel (→ runtime `TypeError: value is not a
//! function`) and `new Function(...)` to an unknown-class `Expr::New`
//! (→ a class_id=0 empty-object placeholder). Neither named *why* the call
//! couldn't compile, and there was no single decision point every later
//! phase of #1677 could build on.
//!
//! This module is that decision point. It buckets each call site into:
//!
//! 1. [`EvalBucket::ConstFoldable`] — the body argument is a compile-time
//!    constant string (string literal / substitution-free template, or no
//!    body at all). Phase 1 (#1679) will compile these to native functions.
//! 2. [`EvalBucket::KnownLibraryCodegen`] — the call originates from a
//!    recognized code-generating library (`fast-json-stringify`, `ajv`,
//!    `find-my-way`). Phases 2–4 (#1680/#1681/#1682) move these to build
//!    time.
//! 3. [`EvalBucket::RuntimeUnknown`] — none of the above; a genuinely
//!    runtime-dynamic code string. This is the only bucket Phase 0 refuses.
//!
//! Phase 0 is pure analysis + reporting: it does **not** compile, fold, or
//! evaluate anything. Buckets 1 and 2 keep their existing (placeholder)
//! lowering so the future phases that own them can swap it out without a
//! behaviour change here; only bucket 3 turns into a hard compile error.
//!
//! `PERRY_EVAL_DIAG=1` logs every classified site (package + `file:line` +
//! bucket) to stderr, so a single compile reveals which dependencies hit
//! each bucket. `PERRY_ALLOW_EVAL=1` downgrades the bucket-3 refusal back
//! to the legacy (non-functional) fall-through for a one-off build — an
//! escape hatch mirroring `#503`'s `PERRY_ALLOW_DYNAMIC_STDLIB`.

use swc_ecma_ast as ast;

/// Which arbitrary-code-execution surface a classified site is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalSurface {
    /// `eval(code)`.
    Eval,
    /// `Function(params..., body)` called without `new`.
    FunctionCall,
    /// `new Function(params..., body)`.
    NewFunction,
}

impl EvalSurface {
    /// Human-readable call shape for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            EvalSurface::Eval => "eval(...)",
            EvalSurface::FunctionCall => "Function(...)",
            EvalSurface::NewFunction => "new Function(...)",
        }
    }
}

/// The classification bucket — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalBucket {
    /// Body is a compile-time-constant string (or absent). → #1679.
    ConstFoldable,
    /// Originates from a recognized codegen library. → #1680/#1681/#1682.
    KnownLibraryCodegen,
    /// Genuinely runtime-dynamic. Refused by Phase 0.
    RuntimeUnknown,
}

impl EvalBucket {
    /// Short tag used in `--diag` log lines.
    pub fn tag(self) -> &'static str {
        match self {
            EvalBucket::ConstFoldable => "const-foldable",
            EvalBucket::KnownLibraryCodegen => "known-library-codegen",
            EvalBucket::RuntimeUnknown => "runtime-unknown",
        }
    }
}

/// npm packages whose `new Function`/`Function(...)`/`eval(...)` calls are
/// recognized as build-time-knowable code generation (the Fastify JIT
/// trio, see #1677). A call from one of these lands in
/// [`EvalBucket::KnownLibraryCodegen`] even when its body is a runtime
/// value, because the *input* to the codegen (a schema, a route table) is
/// build-time-knowable — later phases evaluate them at build time.
pub const KNOWN_CODEGEN_PACKAGES: &[&str] = &["fast-json-stringify", "ajv", "find-my-way"];

/// A classified `eval`/`Function` call site plus its provenance. Pure data
/// — the lowering site decides whether to refuse based on [`Self::bucket`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalClassification {
    /// Which surface (`eval` / `Function` / `new Function`).
    pub surface: EvalSurface,
    /// Which bucket the body argument put this site in.
    pub bucket: EvalBucket,
    /// Originating npm package name, or `None` for user/host source.
    pub package: Option<String>,
    /// Source file the call appears in.
    pub file: String,
    /// 1-based line of the call, or 0 when the source line is unknown.
    pub line: usize,
    /// For const-foldable sites, a short preview of the body string (used
    /// only in `--diag` output). `None` for the other buckets.
    pub body_preview: Option<String>,
}

impl EvalClassification {
    /// Phase 0 refuses exactly the runtime-unknown bucket.
    pub fn is_refused(&self) -> bool {
        self.bucket == EvalBucket::RuntimeUnknown
    }

    /// `file:line` (line omitted when unknown). Built from the call's byte
    /// offset against the currently-installed module source.
    pub fn location(&self) -> String {
        if self.line == 0 {
            self.file.clone()
        } else {
            format!("{}:{}", self.file, self.line)
        }
    }

    /// `(in package `pkg`)` / `(user source)` provenance label.
    pub fn provenance(&self) -> String {
        match &self.package {
            Some(pkg) => format!("in package `{}`", pkg),
            None => "user source".to_string(),
        }
    }

    /// The bucket-3 refusal message: names the surface, `file:line`, the
    /// originating package, and the available remedies. Includes the
    /// location inline so it surfaces regardless of which command renders
    /// the error (the span is also attached by `lower_bail!` for `perry
    /// check`'s snippet emitter).
    pub fn refusal_message(&self) -> String {
        format!(
            "`{surface}` is refused at compile time: {loc} ({prov}). Perry is an \
             ahead-of-time compiler — it cannot evaluate a code string built from \
             runtime data. (#1677)\n\
             \n\
             Options:\n\
             - Replace the generated function with an ordinary function or closure.\n\
             - If the body is a build-time constant string, a future release will \
               compile it natively (#1679).\n\
             - If this comes from a code-generating library, only \
               `fast-json-stringify`, `ajv`, and `find-my-way` are recognized so far \
               (#1680/#1681/#1682) — file an issue against #1677 naming the package.\n\
             - Set `PERRY_ALLOW_EVAL=1` to restore the legacy (non-functional) \
               behavior for a one-off build.",
            surface = self.surface.label(),
            loc = self.location(),
            prov = self.provenance(),
        )
    }

    /// One `--diag` log line: surface, `file:line`, provenance, bucket, and
    /// (for const-foldable sites) a body preview.
    pub fn diag_line(&self) -> String {
        let preview = match &self.body_preview {
            Some(b) => format!(" body={:?}", b),
            None => String::new(),
        };
        format!(
            "[perry-eval-diag] {surface} @ {loc} ({prov}) -> {bucket}{preview}",
            surface = self.surface.label(),
            loc = self.location(),
            prov = self.provenance(),
            bucket = self.bucket.tag(),
        )
    }
}

/// Peel parens and return the constant string value of `expr` if it is a
/// string literal or a substitution-free template literal. `None` for any
/// other shape (a variable, concatenation, call result, …).
///
/// Public so Phase 1 (#1679) const-folding decides constness the *same*
/// way the Phase 0 classifier does — the fold must agree with the bucket.
pub fn const_string_of(expr: &ast::Expr) -> Option<String> {
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.as_str().unwrap_or("").to_string()),
        ast::Expr::Tpl(tpl) if tpl.exprs.is_empty() => {
            // A template with no `${}` substitutions is a constant. Prefer
            // the cooked value (escapes resolved, WTF-8 → may be `None` for
            // a lone surrogate); fall back to the raw text.
            tpl.quasis.first().map(|q| {
                q.cooked
                    .as_ref()
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| q.raw.as_str().to_string())
            })
        }
        _ => None,
    }
}

/// Truncate a body preview so `--diag` lines stay readable.
fn preview(body: &str) -> String {
    const MAX: usize = 48;
    if body.chars().count() > MAX {
        let head: String = body.chars().take(MAX).collect();
        format!("{}…", head)
    } else {
        body.to_string()
    }
}

/// Classify a single `eval`/`Function`/`new Function` call site. Pure
/// analysis — `body_arg` is the code-string argument (the *last* arg for
/// `Function`, the *only* arg for `eval`; `None` when the call has no
/// body argument). `byte_offset` is the call's `span.lo.0`, resolved to a
/// line against the currently-installed module source.
pub fn classify(
    surface: EvalSurface,
    body_arg: Option<&ast::Expr>,
    source_file_path: &str,
    byte_offset: u32,
) -> EvalClassification {
    let package = crate::ir::package_name_for_source_path(source_file_path).map(|s| s.to_string());

    // Bucket 1: const-foldable. A missing body argument is an empty
    // (hence constant) function body, so it folds too.
    let const_body = match body_arg {
        Some(arg) => const_string_of(arg),
        None => Some(String::new()),
    };

    let (bucket, body_preview) = if let Some(body) = &const_body {
        (EvalBucket::ConstFoldable, Some(preview(body)))
    } else if package
        .as_deref()
        .is_some_and(|p| KNOWN_CODEGEN_PACKAGES.contains(&p))
    {
        // Bucket 2: recognized codegen library with a runtime-built body.
        (EvalBucket::KnownLibraryCodegen, None)
    } else {
        // Bucket 3: genuinely runtime-dynamic.
        (EvalBucket::RuntimeUnknown, None)
    };

    EvalClassification {
        surface,
        bucket,
        package,
        file: source_file_path.to_string(),
        line: crate::ir::current_module_line_at(byte_offset).unwrap_or(0),
        body_preview,
    }
}

/// Whether `PERRY_EVAL_DIAG` is set to a truthy value — enables per-site
/// classification logging.
pub fn eval_diag_enabled() -> bool {
    env_flag("PERRY_EVAL_DIAG")
}

/// Whether `PERRY_ALLOW_EVAL` is set — downgrades the bucket-3 refusal to
/// the legacy fall-through for a one-off build.
pub fn eval_override_enabled() -> bool {
    env_flag("PERRY_ALLOW_EVAL")
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "" | "0" | "off" | "false" | "no")
        }
        Err(_) => false,
    }
}

/// Log a classified site under `PERRY_EVAL_DIAG`. No-op otherwise.
pub fn report(classification: &EvalClassification) {
    if eval_diag_enabled() {
        eprintln!("{}", classification.diag_line());
    }
}

/// The single decision point both lowering sites (`new Function` in
/// `expr_new`, `Function(...)`/`eval(...)` in `expr_call`) funnel through.
///
/// Classifies the site, logs it under `PERRY_EVAL_DIAG`, and returns
/// `Err` (a span-tagged [`crate::error::LowerError`]) only for the
/// runtime-unknown bucket — unless `PERRY_ALLOW_EVAL` is set, which
/// downgrades the refusal to the legacy fall-through. `Ok(())` means the
/// caller should proceed with its existing lowering (const-foldable /
/// known-library sites keep their placeholder behaviour for Phase 0).
pub fn check_site(
    surface: EvalSurface,
    body_arg: Option<&ast::Expr>,
    source_file_path: &str,
    span: swc_common::Span,
) -> anyhow::Result<()> {
    let classification = classify(surface, body_arg, source_file_path, span.lo.0);
    report(&classification);
    if classification.is_refused() && !eval_override_enabled() {
        // #2309: when the tree-shake deferral sink is armed (a node_modules
        // module being lowered under tree-shaking), record the refusal and
        // fall through instead of erroring — the module may be pruned as
        // unreachable, in which case this refusal never matters. The driver
        // re-raises any deferred refusal that survives the prune.
        if crate::deferral::try_defer_refusal(classification.refusal_message(), span.lo.0) {
            return Ok(());
        }
        return Err(anyhow::Error::new(crate::error::LowerError::new(
            classification.refusal_message(),
            span,
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{clear_current_module_source, set_current_module_source};
    use swc_common::{BytePos, Span};

    fn str_lit(s: &str) -> ast::Expr {
        ast::Expr::Lit(ast::Lit::Str(ast::Str {
            span: Span::new(BytePos(0), BytePos(0)),
            value: s.into(),
            raw: None,
        }))
    }

    /// A non-constant expression stand-in (any shape `const_string_of`
    /// can't fold) — `Invalid` needs only a span, so it dodges
    /// version-specific `Ident` constructors.
    fn non_const() -> ast::Expr {
        ast::Expr::Invalid(ast::Invalid {
            span: Span::new(BytePos(0), BytePos(0)),
        })
    }

    #[test]
    fn string_literal_body_is_const_foldable() {
        let body = str_lit("return a + b");
        let c = classify(EvalSurface::NewFunction, Some(&body), "/app/main.ts", 0);
        assert_eq!(c.bucket, EvalBucket::ConstFoldable);
        assert!(!c.is_refused());
        assert_eq!(c.package, None);
        assert_eq!(c.body_preview.as_deref(), Some("return a + b"));
    }

    #[test]
    fn absent_body_is_const_foldable() {
        // `new Function()` — empty function body, trivially constant.
        let c = classify(EvalSurface::NewFunction, None, "/app/main.ts", 0);
        assert_eq!(c.bucket, EvalBucket::ConstFoldable);
        assert_eq!(c.body_preview.as_deref(), Some(""));
    }

    #[test]
    fn runtime_body_in_user_source_is_runtime_unknown() {
        let body = non_const();
        let c = classify(EvalSurface::Eval, Some(&body), "/app/main.ts", 0);
        assert_eq!(c.bucket, EvalBucket::RuntimeUnknown);
        assert!(c.is_refused());
        assert_eq!(c.package, None);
        assert!(c.refusal_message().contains("user source"));
        assert!(c.refusal_message().contains("eval(...)"));
    }

    #[test]
    fn runtime_body_in_known_codegen_package_is_known_library() {
        let body = non_const();
        let path = "/proj/node_modules/fast-json-stringify/index.js";
        let c = classify(EvalSurface::NewFunction, Some(&body), path, 0);
        assert_eq!(c.bucket, EvalBucket::KnownLibraryCodegen);
        assert!(!c.is_refused());
        assert_eq!(c.package.as_deref(), Some("fast-json-stringify"));
    }

    #[test]
    fn runtime_body_in_unknown_package_is_runtime_unknown() {
        let body = non_const();
        let path = "/proj/node_modules/sketchy-pkg/dist/x.js";
        let c = classify(EvalSurface::FunctionCall, Some(&body), path, 0);
        assert_eq!(c.bucket, EvalBucket::RuntimeUnknown);
        assert!(c.is_refused());
        assert_eq!(c.package.as_deref(), Some("sketchy-pkg"));
        let msg = c.refusal_message();
        assert!(msg.contains("in package `sketchy-pkg`"));
    }

    #[test]
    fn known_codegen_with_const_body_prefers_const_foldable() {
        // Const body wins over the package match — a literal body is
        // compilable regardless of which package it lives in.
        let body = str_lit("return 1");
        let path = "/proj/node_modules/ajv/dist/x.js";
        let c = classify(EvalSurface::NewFunction, Some(&body), path, 0);
        assert_eq!(c.bucket, EvalBucket::ConstFoldable);
    }

    #[test]
    fn template_without_substitutions_is_const() {
        let body = ast::Expr::Tpl(ast::Tpl {
            span: Span::new(BytePos(0), BytePos(0)),
            exprs: vec![],
            quasis: vec![ast::TplElement {
                span: Span::new(BytePos(0), BytePos(0)),
                tail: true,
                cooked: Some("return 7".into()),
                raw: "return 7".into(),
            }],
        });
        let c = classify(EvalSurface::NewFunction, Some(&body), "/app/main.ts", 0);
        assert_eq!(c.bucket, EvalBucket::ConstFoldable);
        assert_eq!(c.body_preview.as_deref(), Some("return 7"));
    }

    #[test]
    fn line_resolved_from_installed_module_source() {
        // Offset lands on line 3 (two newlines precede it).
        set_current_module_source("a\nb\nnew Function(x)\n".to_string());
        let offset = "a\nb\n".len() as u32;
        let body = non_const();
        let c = classify(
            EvalSurface::NewFunction,
            Some(&body),
            "/app/main.ts",
            offset,
        );
        assert_eq!(c.line, 3);
        assert_eq!(c.location(), "/app/main.ts:3");
        assert!(c.refusal_message().contains("/app/main.ts:3"));
        clear_current_module_source();
    }

    #[test]
    fn long_body_preview_truncated() {
        let long = "x".repeat(100);
        let body = str_lit(&long);
        let c = classify(EvalSurface::NewFunction, Some(&body), "/app/main.ts", 0);
        let p = c.body_preview.unwrap();
        assert!(p.ends_with('…'));
        assert_eq!(p.chars().count(), 49); // 48 chars + ellipsis
    }
}

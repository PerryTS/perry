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
//! each bucket.
//!
//! ## #5206 — deferred-runtime-error default vs. strict refusal
//!
//! As of #5206 the runtime-unknown bucket no longer blocks the build by
//! default. Two compile modes select what happens to a bucket-3 site:
//!
//! - **defer** (the default, non-strict): the site is compiled to a value
//!   that throws a descriptive [`Error`] *only when it is actually
//!   invoked* (an `eval(...)` call throws when evaluated; a `new
//!   Function(...)` returns a function that throws when called). Each such
//!   site is recorded in a thread-local sink so the compile driver can
//!   print a single visible end-of-build notice listing the degraded
//!   sites (count + kind + `file:line`).
//! - **error** (strict, opt-in): restores the historical hard compile-time
//!   refusal — every bucket-3 site fails the build with [`EvalClassification::refusal_message`].
//!
//! The mode is a thread-local set at compile entry from the CLI flag
//! (`--strict-eval`) and project config (`perry.eval` / `perry.strict`).
//! `PERRY_ALLOW_EVAL=1` is kept for back-compat: it forces non-strict
//! (defer) mode and so overrides a strict config/flag for a one-off build,
//! mirroring `#503`'s `PERRY_ALLOW_DYNAMIC_STDLIB`.

use std::cell::RefCell;
use std::sync::Mutex;

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
             - This refusal is strict-eval mode. The default (`perry.eval = \"defer\"`) \
               instead compiles the site to a runtime error that throws only if reached, \
               and prints a compile-time notice. Drop `--strict-eval` / `perry.eval = \
               \"error\"` (or set `PERRY_ALLOW_EVAL=1`) to use it.",
            surface = self.surface.label(),
            loc = self.location(),
            prov = self.provenance(),
        )
    }

    /// The descriptive message a *deferred* bucket-3 site throws at runtime
    /// when it is actually reached (#5206). Names the surface and the
    /// `file:line` so a crash points straight back at the offending source.
    pub fn deferred_runtime_error_message(&self) -> String {
        let what = match self.surface {
            EvalSurface::Eval => "eval()",
            EvalSurface::FunctionCall | EvalSurface::NewFunction => "new Function()",
        };
        format!(
            "{what} cannot run in an ahead-of-time compiled binary ({loc})",
            loc = self.location(),
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
        // Constant string concatenation: `'a' + 'b' + 'c'`. Test262's
        // procedurally-generated eval cases split a body across `+`-joined
        // string literals (one segment per `switch` case / `if` branch), so
        // the whole argument is still a constant the AOT eval fold can run.
        // Only fold when BOTH operands are themselves constant strings — a
        // numeric `+` (or a string + non-constant) is not a constant body.
        ast::Expr::Bin(bin) if bin.op == ast::BinaryOp::Add => {
            let mut left = const_string_of(&bin.left)?;
            let right = const_string_of(&bin.right)?;
            left.push_str(&right);
            Some(left)
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

/// Whether `PERRY_ALLOW_EVAL` is set — forces non-strict (defer) mode for a
/// one-off build, overriding any strict flag/config (back-compat with the
/// pre-#5206 escape hatch).
pub fn eval_override_enabled() -> bool {
    env_flag("PERRY_ALLOW_EVAL")
}

/// Whether to present runtime dynamic-code generation as *unavailable* to
/// capability probes. A trivial no-op `new Function("")` / `Function("")` is the
/// canonical dynamic-codegen feature-test (`try { new Function(""), true } catch
/// { false }` — the same shape a CSP `unsafe-eval` policy blocks). When this is
/// on, that probe throws at *construction* instead of const-folding to a no-op
/// function, so feature-detecting libraries (e.g. zod 4's object-validator JIT)
/// take their non-codegen interpreter fallback, which perry compiles normally.
///
/// **Default ON.** Perry is ahead-of-time compiled and can NEVER honor a runtime
/// `new Function(<string built at runtime>)` — such a call throws at
/// construction (`js_function_ctor_from_strings`). Folding the empty-body probe
/// to a working no-op makes the capability probe *lie*: a JIT then enables its
/// codegen path and throws on the first real dynamic body (uncaught, this rejects
/// the surrounding async work). Reporting the capability honestly as unavailable
/// is both more correct for an AOT binary and lets such libraries run. Opt out —
/// restoring the spec-literal empty-function fold (Node returns an empty
/// function) — with `PERRY_EVAL_CSP=0` (also `off`/`false`/`no`). Only the
/// trivial no-op probe is affected: real literal bodies (`new Function("return
/// 42")`, the `return this` globalThis polyfill) still fold, so those idioms are
/// unaffected.
pub fn eval_csp_probe_unavailable() -> bool {
    match std::env::var("PERRY_EVAL_CSP") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        ),
        Err(_) => true,
    }
}

/// Whether `PERRY_ALLOW_UNIMPLEMENTED` is set — forces non-strict (defer) mode
/// for recognized-but-unimplemented node/stdlib APIs (#5245), overriding any
/// strict flag/config for a one-off build. This is the back-compat alias of the
/// pre-#5245 `#463` escape hatch: it used to skip the gate entirely (silent
/// fall-through); it now force-defers the site to a throw-on-reach runtime error
/// AND records it in the end-of-compile notice, like the default path.
pub fn unimplemented_override_enabled() -> bool {
    std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some()
}

thread_local! {
    /// `true` when strict-eval mode is active for the current compile: a
    /// runtime-unknown (`bucket-3`) site is a hard compile-time refusal.
    /// `false` (the default) defers it to a throw-on-reach runtime error.
    /// Set once at compile entry (and re-applied per rayon worker) via
    /// [`set_eval_strict_mode`].
    static EVAL_STRICT_MODE: RefCell<bool> = const { RefCell::new(false) };

    /// `true` when strict-unimplemented mode is active for the current compile
    /// (#5245): a reference to a recognized-but-unimplemented node/stdlib API is
    /// a hard compile-time refusal (the historical `#463` behavior). `false`
    /// (the default) defers it to a throw-on-reach runtime error and records the
    /// site for the end-of-compile notice. Set once at compile entry (and
    /// re-applied per rayon worker) via [`set_unimplemented_strict_mode`].
    static UNIMPL_STRICT_MODE: RefCell<bool> = const { RefCell::new(false) };
}

/// Sites that were deferred to a runtime error during this compile (#5206).
/// Process-global (not thread-local) because modules lower on rayon worker
/// threads while the driver drains this at the end of the build to print the
/// visible "degraded sites" notice. De-duplicated by `(kind, location)` so a
/// module lowered more than once isn't counted twice.
static EVAL_DEFERRED_SITES: Mutex<Vec<DeferredEvalSite>> = Mutex::new(Vec::new());

/// A site that was compiled to a deferred throw-on-reach runtime error
/// instead of failing the build. Reported in the end-of-compile notice.
///
/// #5206 introduced this for runtime-unknown `eval(...)` / `new Function(...)`
/// sites; #5230 generalized it to any ahead-of-time-unsupported surface
/// (currently also a non-resolvable dynamic `import(...)`). The `kind` string
/// distinguishes the surface (`eval(...)`, `new Function(...)`, `import(...)`)
/// so a single notice can list every degraded site of every kind together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredEvalSite {
    /// Display label of the surface, e.g. `new Function(...)` or `import(...)`.
    pub kind: String,
    /// `file:line` of the site.
    pub location: String,
}

/// Set strict-eval mode for the current compile thread. `true` restores the
/// historical hard compile-time refusal of runtime-unknown sites; `false`
/// (the default) defers them to throw-on-reach runtime errors. Called once
/// at compile entry. `PERRY_ALLOW_EVAL` always wins (forces `false`).
pub fn set_eval_strict_mode(strict: bool) {
    EVAL_STRICT_MODE.with(|s| *s.borrow_mut() = strict && !eval_override_enabled());
}

/// Whether strict-eval mode is active for the current compile.
pub fn eval_strict_mode() -> bool {
    EVAL_STRICT_MODE.with(|s| *s.borrow())
}

/// Set strict-unimplemented mode for the current compile thread (#5245).
/// `true` restores the historical hard `#463` compile-time refusal of a
/// recognized-but-unimplemented node/stdlib API; `false` (the default) defers
/// it to a throw-on-reach runtime error. Called once at compile entry.
/// `PERRY_ALLOW_UNIMPLEMENTED` always wins (forces `false`).
pub fn set_unimplemented_strict_mode(strict: bool) {
    UNIMPL_STRICT_MODE.with(|s| *s.borrow_mut() = strict && !unimplemented_override_enabled());
}

/// Whether strict-unimplemented mode is active for the current compile.
pub fn unimplemented_strict_mode() -> bool {
    UNIMPL_STRICT_MODE.with(|s| *s.borrow())
}

/// `kind` tag used for unimplemented-API sites in the shared end-of-compile
/// notice (#5245). Kept as a constant so the notice and any tests agree.
pub const UNIMPLEMENTED_API_KIND: &str = "unimplemented API";

/// What a lowering site should do with a recognized-but-unimplemented
/// node/stdlib API reference (#5245). Mirrors [`EvalDecision`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnimplementedDecision {
    /// Strict-unimplemented mode: fail the build with the `#463` refusal.
    Refuse,
    /// Default (defer) mode, the `PERRY_ALLOW_UNIMPLEMENTED` override, or a
    /// tree-shake deferral: compile the reference to a value that throws this
    /// descriptive message only if it is actually reached.
    DeferToRuntimeError(String),
}

/// Resolve a `file:line` location from a source path + byte offset, the same
/// way [`EvalClassification::location`] does (line omitted when unknown). Used
/// by the unimplemented-API gate sites (#5245), which don't build a full
/// [`EvalClassification`].
pub fn location_string(source_file_path: &str, byte_offset: u32) -> String {
    match crate::ir::current_module_line_at(byte_offset) {
        Some(line) if line != 0 => format!("{source_file_path}:{line}"),
        _ => source_file_path.to_string(),
    }
}

/// Build the descriptive message a *deferred* unimplemented-API site throws at
/// runtime when it is actually reached (#5245). `api` is the dotted display
/// label (e.g. `process.binding`); `location` is `file:line`.
pub fn unimplemented_runtime_error_message(api: &str, location: &str) -> String {
    format!("{api} is not implemented in Perry (ahead-of-time) ({location})")
}

/// The single decision point every unimplemented-API gate (#463 sites in
/// `expr_member` / `expr_call`) funnels through (#5245).
///
/// - **strict-unimplemented** mode → [`UnimplementedDecision::Refuse`]: the
///   caller raises the historical `#463` compile-time error.
/// - **default** (defer) mode, the `PERRY_ALLOW_UNIMPLEMENTED` override, or an
///   armed tree-shake deferral sink → records the site for the end-of-compile
///   notice (except the tree-shake path, which records its own deferral) and
///   returns [`UnimplementedDecision::DeferToRuntimeError`] with the message the
///   caller should compile to a throw-on-reach value.
///
/// `refusal_message` is the full `#463` message (used for the strict error and
/// the tree-shake deferral sink). `api` is the dotted API label and `location`
/// is `file:line` — both feed the deferred runtime message and the notice.
/// `byte_offset` is the site's `span.lo.0` (for the tree-shake sink).
pub fn check_unimplemented_api(
    refusal_message: &str,
    api: &str,
    location: &str,
    byte_offset: u32,
) -> UnimplementedDecision {
    let runtime_msg = unimplemented_runtime_error_message(api, location);

    // Tree-shake deferral (mirrors `check_site`): when the sink is armed for a
    // node_modules module lowered under tree-shaking, record the refusal and
    // compile to the throw-on-reach value — the module may be pruned. The
    // driver re-raises a surviving deferral (strict) or surfaces it (defer).
    if crate::deferral::try_defer_refusal(refusal_message.to_string(), byte_offset) {
        return UnimplementedDecision::DeferToRuntimeError(runtime_msg);
    }

    if unimplemented_strict_mode() {
        return UnimplementedDecision::Refuse;
    }

    // Default (defer) mode or the `PERRY_ALLOW_UNIMPLEMENTED` override: record
    // the site for the visible end-of-compile notice and defer.
    record_deferred_aot_site(UNIMPLEMENTED_API_KIND, location);
    UnimplementedDecision::DeferToRuntimeError(runtime_msg)
}

/// Record a deferred ahead-of-time-unsupported site for the end-of-compile
/// notice. Shared sink for every degraded surface (#5206 eval / `new Function`,
/// #5230 dynamic `import(...)`). Idempotent per `(kind, location)` so the same
/// site lowered more than once (rayon re-lower, two passes) isn't double-counted.
///
/// `kind` is the display label of the surface (e.g. `import(...)`); `location`
/// is `file:line`.
pub fn record_deferred_aot_site(kind: impl Into<String>, location: impl Into<String>) {
    let site = DeferredEvalSite {
        kind: kind.into(),
        location: location.into(),
    };
    if let Ok(mut v) = EVAL_DEFERRED_SITES.lock() {
        if !v.contains(&site) {
            v.push(site);
        }
    }
}

/// Record a deferred bucket-3 eval/Function site for the end-of-compile notice.
fn record_deferred_site(classification: &EvalClassification) {
    record_deferred_aot_site(
        classification.surface.label().to_string(),
        classification.location(),
    );
}

/// Drain and return every deferred bucket-3 site recorded so far this
/// compile. Called by the driver to render the end-of-compile notice.
pub fn take_deferred_eval_sites() -> Vec<DeferredEvalSite> {
    EVAL_DEFERRED_SITES
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

/// What the lowering site should do with a classified call (#5206).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalDecision {
    /// Const-foldable / known-library site: proceed with the existing
    /// (placeholder / native-fold) lowering unchanged.
    Proceed,
    /// Runtime-unknown site under the default (defer) mode, or a tree-shake
    /// deferral: compile it to a value that throws this descriptive [`Error`]
    /// message only when reached.
    DeferToRuntimeError(String),
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
/// Classifies the site, logs it under `PERRY_EVAL_DIAG`, and decides what the
/// caller does with it (#5206):
///
/// - const-foldable / known-library buckets → [`EvalDecision::Proceed`]
///   (existing lowering unchanged).
/// - runtime-unknown bucket in **strict** mode → `Err` (a span-tagged
///   [`crate::error::LowerError`]) — the historical hard refusal.
/// - runtime-unknown bucket in the **default** (defer) mode → records the
///   site for the end-of-compile notice and returns
///   [`EvalDecision::DeferToRuntimeError`] with the message the caller should
///   compile to a throw-on-reach value.
///
/// The tree-shake deferral sink (#2309) still short-circuits in either mode:
/// when armed for a `node_modules` module under tree-shaking, the refusal is
/// recorded and deferred (the module may be pruned), and the call lowers to
/// the throw-on-reach value so a *surviving* module still behaves correctly
/// while the driver re-raises (strict) or notices (defer) it.
pub fn check_site(
    surface: EvalSurface,
    body_arg: Option<&ast::Expr>,
    source_file_path: &str,
    span: swc_common::Span,
) -> anyhow::Result<EvalDecision> {
    let classification = classify(surface, body_arg, source_file_path, span.lo.0);
    report(&classification);
    if !classification.is_refused() {
        return Ok(EvalDecision::Proceed);
    }

    let strict = eval_strict_mode();

    // #2309: tree-shake deferral. When the sink is armed (a node_modules
    // module lowered under tree-shaking), record the refusal and compile to
    // the throw-on-reach value instead of erroring — the module may be pruned
    // as unreachable. The driver re-raises any deferred refusal that survives
    // the prune (strict), or surfaces it in the notice (defer).
    if crate::deferral::try_defer_refusal(classification.refusal_message(), span.lo.0) {
        return Ok(EvalDecision::DeferToRuntimeError(
            classification.deferred_runtime_error_message(),
        ));
    }

    if strict {
        return Err(anyhow::Error::new(crate::error::LowerError::new(
            classification.refusal_message(),
            span,
        )));
    }

    // Default (defer) mode: throw-on-reach + visible end-of-compile notice.
    record_deferred_site(&classification);
    Ok(EvalDecision::DeferToRuntimeError(
        classification.deferred_runtime_error_message(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{clear_current_module_source, set_current_module_source};
    use std::sync::Mutex;
    use swc_common::{BytePos, Span};

    /// Serializes the tests that drain the process-global deferred-eval-site
    /// sink. `take_deferred_eval_sites()` is destructive (it drains the WHOLE
    /// sink), so two such tests running concurrently under the parallel
    /// `cargo test` harness steal each other's recorded sites — which flaked
    /// `unimplemented_defers_by_default_and_records_site` ("exactly one recorded
    /// site"). Each sink-touching test holds this lock across its push→take.
    static EVAL_SITE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire [`EVAL_SITE_TEST_LOCK`], tolerating poisoning from an unrelated
    /// panicking test — we only need the mutual exclusion, not protected data.
    fn lock_eval_sink() -> std::sync::MutexGuard<'static, ()> {
        EVAL_SITE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

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
    fn constant_string_concatenation_folds() {
        // Test262's procedurally-generated eval cases split a body across
        // `+`-joined string literals; the whole argument is still a constant.
        let add = |l: ast::Expr, r: ast::Expr| {
            ast::Expr::Bin(ast::BinExpr {
                span: Span::new(BytePos(0), BytePos(0)),
                op: ast::BinaryOp::Add,
                left: Box::new(l),
                right: Box::new(r),
            })
        };
        // `'switch (1) {' + '  case 1:' + '}'`
        let expr = add(
            add(str_lit("switch (1) {"), str_lit("  case 1:")),
            str_lit("}"),
        );
        assert_eq!(
            const_string_of(&expr).as_deref(),
            Some("switch (1) {  case 1:}")
        );
        // A non-`+` operator, or a non-constant operand, does not fold.
        let sub = ast::Expr::Bin(ast::BinExpr {
            span: Span::new(BytePos(0), BytePos(0)),
            op: ast::BinaryOp::Sub,
            left: Box::new(str_lit("a")),
            right: Box::new(str_lit("b")),
        });
        assert_eq!(const_string_of(&sub), None);
        assert_eq!(const_string_of(&add(str_lit("a"), non_const())), None);
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

    #[test]
    fn deferred_runtime_error_message_names_surface_and_location() {
        let body = non_const();
        let c = classify(EvalSurface::NewFunction, Some(&body), "/app/x.ts", 0);
        let msg = c.deferred_runtime_error_message();
        assert!(msg.contains("new Function()"));
        assert!(msg.contains("ahead-of-time compiled binary"));
        assert!(msg.contains("/app/x.ts"));

        let body = non_const();
        let c = classify(EvalSurface::Eval, Some(&body), "/app/x.ts", 0);
        assert!(c.deferred_runtime_error_message().contains("eval()"));
    }

    /// Default (non-strict) mode: a runtime-unknown site defers to a
    /// throw-on-reach value AND is recorded for the end-of-compile notice.
    #[test]
    fn default_mode_defers_runtime_unknown_and_records_site() {
        let _sink_guard = lock_eval_sink();
        set_eval_strict_mode(false);
        // Use a unique path so this test's recorded site is identifiable even
        // if other tests push to the process-global sink concurrently.
        let path = "/app/default_mode_defers_fixture.ts";
        let span = Span::new(BytePos(0), BytePos(0));
        let body = non_const();
        let decision = check_site(EvalSurface::Eval, Some(&body), path, span).expect("no error");
        match decision {
            EvalDecision::DeferToRuntimeError(msg) => {
                assert!(msg.contains("eval()"));
                assert!(msg.contains(path));
            }
            other => panic!("expected defer, got {other:?}"),
        }
        let sites = take_deferred_eval_sites();
        let mine: Vec<_> = sites.iter().filter(|s| s.location.contains(path)).collect();
        assert_eq!(mine.len(), 1, "exactly one recorded site for {path}");
        assert_eq!(mine[0].kind, "eval(...)");
    }

    /// Strict-eval mode: a runtime-unknown site is a hard compile-time error.
    #[test]
    fn strict_mode_refuses_runtime_unknown() {
        let _sink_guard = lock_eval_sink();
        // PERRY_ALLOW_EVAL would force non-strict; only assert when unset.
        if eval_override_enabled() {
            return;
        }
        set_eval_strict_mode(true);
        let span = Span::new(BytePos(0), BytePos(0));
        let body = non_const();
        let path = "/app/strict_refuses_fixture.ts";
        let res = check_site(EvalSurface::Eval, Some(&body), path, span);
        assert!(res.is_err(), "strict mode must refuse runtime-unknown");
        // No notice site recorded for this path in strict mode.
        assert!(
            !take_deferred_eval_sites()
                .iter()
                .any(|s| s.location.contains(path)),
            "strict mode must not record a notice site"
        );
        set_eval_strict_mode(false); // restore for sibling tests on this thread
    }

    /// Const-foldable sites always proceed regardless of mode.
    #[test]
    fn const_foldable_always_proceeds() {
        set_eval_strict_mode(true);
        let span = Span::new(BytePos(0), BytePos(0));
        let body = str_lit("return 1");
        let decision = check_site(EvalSurface::NewFunction, Some(&body), "/app/main.ts", span)
            .expect("const-foldable never errors");
        assert_eq!(decision, EvalDecision::Proceed);
        set_eval_strict_mode(false);
    }

    /// `PERRY_ALLOW_EVAL` forces non-strict even when strict is requested.
    #[test]
    fn allow_eval_env_forces_non_strict() {
        // Only meaningful when the env var is actually set; otherwise the
        // back-compat alias has nothing to override and we skip.
        if !eval_override_enabled() {
            return;
        }
        set_eval_strict_mode(true);
        assert!(
            !eval_strict_mode(),
            "PERRY_ALLOW_EVAL must force non-strict"
        );
    }

    // ---- #5245: unimplemented-API gate decision ----

    /// Default (non-strict) mode: an unimplemented-API site defers to a
    /// throw-on-reach value AND is recorded for the end-of-compile notice with
    /// the `"unimplemented API"` kind.
    #[test]
    fn unimplemented_defers_by_default_and_records_site() {
        let _sink_guard = lock_eval_sink();
        // PERRY_ALLOW_UNIMPLEMENTED forces defer regardless — fine for this
        // (defer) assertion either way, so no skip needed.
        set_unimplemented_strict_mode(false);
        // Unique location so this test's recorded site is identifiable even if
        // sibling tests push to the process-global sink concurrently.
        let loc = "/app/unimpl_defers_fixture.ts:42";
        let decision = check_unimplemented_api(
            "`process.binding` is not implemented (#463)",
            "process.binding",
            loc,
            0,
        );
        match decision {
            UnimplementedDecision::DeferToRuntimeError(msg) => {
                assert!(msg.contains("process.binding"), "msg: {msg}");
                assert!(msg.contains("ahead-of-time"), "msg: {msg}");
                assert!(msg.contains(loc), "msg: {msg}");
            }
            other => panic!("expected defer, got {other:?}"),
        }
        let sites = take_deferred_eval_sites();
        let mine: Vec<_> = sites.iter().filter(|s| s.location == loc).collect();
        assert_eq!(mine.len(), 1, "exactly one recorded site for {loc}");
        assert_eq!(mine[0].kind, UNIMPLEMENTED_API_KIND);
    }

    /// Strict-unimplemented mode: an unimplemented-API site is refused (the
    /// caller raises the hard `#463` error) and records no notice site.
    #[test]
    fn unimplemented_refuses_in_strict_mode() {
        let _sink_guard = lock_eval_sink();
        // PERRY_ALLOW_UNIMPLEMENTED would force defer; only assert when unset.
        if unimplemented_override_enabled() {
            return;
        }
        set_unimplemented_strict_mode(true);
        let loc = "/app/unimpl_strict_fixture.ts:7";
        let decision = check_unimplemented_api("refusal", "fs.bogus", loc, 0);
        assert_eq!(decision, UnimplementedDecision::Refuse);
        assert!(
            !take_deferred_eval_sites().iter().any(|s| s.location == loc),
            "strict mode must not record a notice site"
        );
        set_unimplemented_strict_mode(false); // restore for sibling tests
    }

    /// `PERRY_ALLOW_UNIMPLEMENTED` forces non-strict even when strict is set.
    #[test]
    fn allow_unimplemented_env_forces_non_strict() {
        if !unimplemented_override_enabled() {
            return;
        }
        set_unimplemented_strict_mode(true);
        assert!(
            !unimplemented_strict_mode(),
            "PERRY_ALLOW_UNIMPLEMENTED must force non-strict"
        );
    }
}

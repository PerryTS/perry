//! Compile-time `PERRY_VERIFY_FACTS=1` diagnostic instrumentation.
//!
//! Existing witnesses use borrowed static metadata, while known-inactive
//! emitted bodies trap at entry. The compile-time coverage manifest names
//! consumed predicates whose checks belong to later slices. Passing checks
//! preserve evaluated operands; failure writes one JSON line and exits 86.
//! Without the mode (including `=0`), metadata, body annotations and coverage
//! recording remain behind the existing gate. Contract builds retain their
//! legacy witness ABI.
use std::cell::{Cell, RefCell};
use std::sync::{Mutex, OnceLock};
mod coverage;
pub(crate) mod life;
pub(crate) mod life_measure;
pub(crate) mod zero_sign;
pub use coverage::take_manifest;
pub(crate) use coverage::{consumed, Fact};
#[cfg(feature = "llvm-inprocess")]
pub(crate) use coverage::{emitted, optimized};

/// A solver site identity, without a second derivation or display-string parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteClaim {
    pub ordinal: usize,
    pub module: String,
    pub byte_offset: Option<u32>,
    pub key: String,
    pub is_call: bool,
    pub bodies: Vec<(u32, String)>,
    pub verdict: SiteVerdict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteVerdict {
    OwnLength,
    NativeMember,
    T0Mono,
    T0Set,
    E0,
}
impl SiteVerdict {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::OwnLength => "OwnLength",
            Self::NativeMember => "NativeMember",
            Self::T0Mono => "T0Mono",
            Self::T0Set => "T0Set",
            Self::E0 => "E0",
        }
    }
    pub(crate) fn slice(self) -> u8 {
        match self {
            Self::OwnLength => 1,
            Self::NativeMember => 2,
            Self::T0Mono => 3,
            Self::T0Set => 4,
            Self::E0 => 5,
        }
    }
}

pub fn register_site_claims(claims: impl IntoIterator<Item = SiteClaim>) {
    if enabled() {
        coverage::sites(claims);
    }
}

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{LlvmType, DOUBLE, I64};

/// The facts-verification build was asked for.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_VERIFY_FACTS").as_deref() == Ok("1"))
}

static INSTRUMENTED_VARIANT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The compile links the instrumented runtime variant (the answer of
/// `optimized_libs::instrumented_variant_requested`, set once by the compile
/// command before codegen). Off until set.
pub fn set_instrumented_variant(on: bool) {
    INSTRUMENTED_VARIANT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The lifetime claims are on: the facts witnesses are on AND the instrumented
/// runtime variant is linked, the only one carrying the collector's life
/// hooks. `PERRY_VERIFY_FACTS=1` alone emits no life claim and links the
/// default archives. The one predicate of codegen and the compile command.
pub fn life_claims_on() -> bool {
    enabled() && INSTRUMENTED_VARIANT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The lifetime measurement is on (LIFE I1, `PERRY_LIFE_MEASURE` set at
/// compile time): an instrument knob, so the compile links the instrumented
/// runtime variant, the only one carrying `perry_life_*`. Codegen emits the
/// birth, activation and loop hooks (`life_measure`) only under it.
pub fn life_measure_on() -> bool {
    life_measure_requested() && INSTRUMENTED_VARIANT.load(std::sync::atomic::Ordering::Relaxed)
}

/// `PERRY_LIFE_MEASURE` is set (non-empty) in the compile's environment.
pub fn life_measure_requested() -> bool {
    std::env::var_os("PERRY_LIFE_MEASURE").is_some_and(|v| !v.is_empty())
}

/// Dead-body removal was asked for (`PERRY_FACTS_DCE=1`, off by default): a
/// body the whole-program reach calls inactive is emitted as a trap.
pub fn dce_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_FACTS_DCE").as_deref() == Ok("1"))
}

/// The facts-consumed lowerings carry their witnesses: this build, or a
/// contract build.
pub(crate) fn witnesses() -> bool {
    enabled() || crate::codegen::helpers::check_contracts()
}

struct Module {
    name: String,
    line_starts: Vec<usize>,
    prefix_lines: u32,
    raw: Vec<String>,
    ordinal: u64,
    site_ordinal: u64,
    copy: String,
}

/// The module's body classifications: its name and `(id, synthetic name)`
/// to active. Both the claim check and the removal read this one map.
type Bodies = (
    String,
    std::sync::Arc<std::collections::BTreeMap<(u32, String), bool>>,
);

thread_local! {
    static MODULE: RefCell<Option<Module>> = const { RefCell::new(None) };
    static BODIES: RefCell<Option<Bodies>> = const { RefCell::new(None) };
    /// The innermost source site being lowered: SWC 1-based start, kind.
    static SITE: Cell<Option<(u32, &'static str)>> = const { Cell::new(None) };
    static COPY_SITE: Cell<u64> = const { Cell::new(0) };
    static KEY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The module being lowered: its name and source, to name a site
/// `file:line:col`.
pub(crate) fn begin_module(name: &str, source: Option<&str>, prefix_lines: u32) {
    if !witnesses() {
        return;
    }
    let line_starts = source
        .map(|s| {
            std::iter::once(0)
                .chain(
                    s.bytes()
                        .enumerate()
                        .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
                )
                .collect()
        })
        .unwrap_or_default();
    MODULE.with(|m| {
        *m.borrow_mut() = Some(Module {
            name: name.to_owned(),
            line_starts,
            prefix_lines,
            raw: Vec::new(),
            ordinal: 0,
            site_ordinal: 0,
            copy: "<layout planning>".to_owned(),
        })
    });
    SITE.with(|s| s.set(None));
}

pub(crate) struct BodyGuard {
    copy: String,
    site: Option<(u32, &'static str)>,
    site_copy: u64,
    key: String,
}

pub(crate) fn body_scope(copy: &str) -> Option<BodyGuard> {
    if !enabled() {
        return None;
    }
    let outer = MODULE.with(|m| {
        let mut m = m.borrow_mut();
        let m = m.as_mut().expect("verification module initialized");
        std::mem::replace(&mut m.copy, copy.to_owned())
    });
    Some(BodyGuard {
        copy: outer,
        site: SITE.with(|s| s.replace(None)),
        site_copy: COPY_SITE.with(|s| s.replace(0)),
        key: KEY.with(|k| k.replace(String::new())),
    })
}

impl Drop for BodyGuard {
    fn drop(&mut self) {
        MODULE.with(|m| {
            if let Some(m) = m.borrow_mut().as_mut() {
                m.copy = std::mem::take(&mut self.copy);
            }
        });
        SITE.with(|s| s.set(self.site));
        COPY_SITE.with(|s| s.set(self.site_copy));
        KEY.with(|k| *k.borrow_mut() = std::mem::take(&mut self.key));
    }
}

pub(crate) struct SiteGuard {
    outer: Option<(u32, &'static str)>,
    outer_copy: Option<u64>,
    outer_key: Option<String>,
    outer_body: Option<String>,
}

/// Lowering `e` starts: a source site (a call, read, write) becomes the
/// innermost site until the guard drops.
pub(crate) fn enter(e: &Expr, copy: &str) -> Option<SiteGuard> {
    if !witnesses() {
        return None;
    }
    let outer_body = if enabled() {
        MODULE.with(|m| {
            if let Some(m) = m.borrow_mut().as_mut() {
                let outer = m.copy.clone();
                if m.copy != copy {
                    m.copy = copy.to_owned();
                }
                return Some(outer);
            }
            None
        })
    } else {
        None
    };
    let here = crate::facts_consumed::site_of(e).map(|(s, _, k)| (s, k));
    let outer = SITE.with(|s| s.get());
    if here.is_some() {
        SITE.with(|s| s.set(here));
    }
    let (outer_copy, outer_key) = if enabled() {
        let ordinal = MODULE.with(|m| {
            let mut m = m.borrow_mut();
            let m = m.as_mut().unwrap();
            m.site_ordinal += 1;
            m.site_ordinal
        });
        (
            Some(COPY_SITE.with(|s| s.replace(ordinal))),
            Some(KEY.with(|k| k.replace(source_key(e)))),
        )
    } else {
        (None, None)
    };
    Some(SiteGuard {
        outer,
        outer_copy,
        outer_key,
        outer_body,
    })
}

impl Drop for SiteGuard {
    fn drop(&mut self) {
        SITE.with(|s| s.set(self.outer));
        if let Some(outer) = self.outer_copy {
            COPY_SITE.with(|s| s.set(outer));
        }
        if let Some(outer) = self.outer_key.take() {
            KEY.with(|s| *s.borrow_mut() = outer);
        }
        if let Some(outer) = self.outer_body.take() {
            MODULE.with(|m| {
                if let Some(m) = m.borrow_mut().as_mut() {
                    m.copy = outer;
                }
            });
        }
    }
}

/// `file:line:col kind` of the innermost site (`file:?` without a source
/// offset or text).
pub(crate) fn here() -> String {
    let site = SITE.with(|s| s.get());
    MODULE.with(|m| {
        let m = m.borrow();
        let name = m.as_ref().map(|m| m.name.as_str()).unwrap_or("?");
        let Some((start, kind)) = site else {
            return format!("{name}:?");
        };
        let off = start.saturating_sub(1) as usize;
        match m.as_ref().filter(|m| !m.line_starts.is_empty()) {
            Some(m) => {
                let line = m.line_starts.partition_point(|&s| s <= off);
                let col = off - m.line_starts[line - 1] + 1;
                let line = (line as u32).saturating_sub(m.prefix_lines);
                format!("{name}:{line}:{col} {kind}")
            }
            None => format!("{name}:@{start} {kind}"),
        }
    })
}

/// The site argument of a witness call: the handle of an interned string
/// naming the site and what its lowering assumes (`here()` + `what`).
pub(crate) fn site_arg(ctx: &mut FnCtx<'_>, what: &str, fact: Fact, receivers: &[u32]) -> SiteArg {
    if enabled() {
        let predicate = if matches!(fact, Fact::Bodies) {
            None
        } else {
            Some(fact.name())
        };
        return raw_site_inner(ctx, fact.name(), None, receivers, what, predicate);
    }
    let text = format!("{} {what}", here());
    let idx = ctx.strings.intern(&text);
    let global = format!("@{}", ctx.strings.entry(idx).handle_global);
    let blk = ctx.block();
    let boxed = blk.load(DOUBLE, &global);
    let bits = blk.bitcast_double_to_i64(&boxed);
    SiteArg {
        pointer: blk.and(I64, &bits, POINTER_MASK_I64),
        len: None,
    }
}

/// Raw metadata operands remain paired through every witness emitter.
pub(crate) struct SiteArg {
    pub pointer: String,
    pub len: Option<String>,
}
impl std::ops::Deref for SiteArg {
    type Target = str;
    fn deref(&self) -> &str {
        &self.pointer
    }
}

pub(crate) fn raw_bytes(prefix: &str, bytes: &[u8]) -> (String, String) {
    MODULE.with(|m| {
        let mut m = m.borrow_mut();
        let m = m.as_mut().expect("verification module initialized");
        let ordinal = m.ordinal;
        m.ordinal += 1;
        let symbol = format!("{prefix}__verify_{ordinal}");
        let escaped: String = bytes.iter().map(|b| format!("\\{b:02X}")).collect();
        m.raw.push(format!(
            "@{symbol} = private constant [{} x i8] c\"{escaped}\", align 1",
            bytes.len()
        ));
        (
            format!("ptrtoint (ptr @{symbol} to i64)"),
            bytes.len().to_string(),
        )
    })
}

pub(crate) fn raw_site(
    ctx: &mut FnCtx<'_>,
    verdict: &str,
    key: Option<&str>,
    receivers: &[u32],
    expectation: &str,
) -> SiteArg {
    raw_site_inner(ctx, verdict, key, receivers, expectation, None)
}

fn raw_site_inner(
    ctx: &mut FnCtx<'_>,
    verdict: &str,
    key: Option<&str>,
    receivers: &[u32],
    expectation: &str,
    predicate: Option<&str>,
) -> SiteArg {
    let (name, offset, ordinal) = MODULE.with(|m| {
        let m = m.borrow();
        let m = m.as_ref().expect("verification module initialized");
        (
            m.name.clone(),
            SITE.with(|s| s.get().map(|s| s.0.saturating_sub(1))),
            m.ordinal,
        )
    });
    let key = key
        .map(str::to_owned)
        .unwrap_or_else(|| KEY.with(|k| k.borrow().clone()));
    let mut metadata = serde_json::json!({
        "marker": "PERRY_FALSE_PROOF", "module": name, "byte_offset": offset,
        "key": key, "verdict": verdict,
        "proven_receivers": receivers,
        "copy": {"body": ctx.func.name, "site":COPY_SITE.with(|s| s.get()), "emission": ordinal},
        "compile_sha": option_env!("PERRY_BUILD_GIT_SHA").unwrap_or("unknown"),
        "expectation": expectation,
    });
    if let Some(predicate) = predicate {
        metadata["predicate"] = serde_json::json!(predicate);
    }
    let mut bytes = serde_json::to_vec(&metadata).expect("static verification metadata");
    assert_eq!(bytes.pop(), Some(b'}'));
    let (pointer, len) = raw_bytes(ctx.strings.module_prefix(), &bytes);
    SiteArg {
        pointer,
        len: Some(len),
    }
}

/// The witness site of code lowered outside any body context (P2's boxed
/// entry, DESIGN-p2 §5): the metadata `raw_site_inner` writes, with the
/// emitting symbol as the copy and no source offset.
pub(crate) fn raw_entry_site(prefix: &str, copy: &str, fact: Fact, what: &str) -> SiteArg {
    let (name, ordinal) = MODULE.with(|m| {
        let m = m.borrow();
        let m = m.as_ref().expect("verification module initialized");
        (m.name.clone(), m.ordinal)
    });
    let metadata = serde_json::json!({
        "marker": "PERRY_FALSE_PROOF", "module": name, "byte_offset": null,
        "key": what, "verdict": fact.name(), "proven_receivers": [],
        "copy": {"body": copy, "site": null, "emission": ordinal},
        "compile_sha": option_env!("PERRY_BUILD_GIT_SHA").unwrap_or("unknown"),
        "expectation": what, "predicate": fact.name(),
    });
    let mut bytes = serde_json::to_vec(&metadata).expect("static verification metadata");
    assert_eq!(bytes.pop(), Some(b'}'));
    let (pointer, len) = raw_bytes(prefix, &bytes);
    SiteArg {
        pointer,
        len: Some(len),
    }
}

pub(crate) fn check(
    ctx: &mut FnCtx<'_>,
    result: LlvmType,
    legacy: &str,
    args: &[(LlvmType, &str)],
    site: &SiteArg,
) -> String {
    let mut operands = args.to_vec();
    operands.push((I64, &site.pointer));
    let name = if let Some(len) = &site.len {
        operands.push((I64, len));
        match legacy {
            "perry_wpfacts_check_call" => "perry_verify_facts_call",
            "perry_wpfacts_check_call_miss" => "perry_verify_facts_call_miss",
            "perry_wpfacts_check_miss" => "perry_verify_facts_miss",
            "perry_wpfacts_check_record_identity" => "perry_verify_facts_record_identity",
            _ => unreachable!("verification ABI"),
        }
    } else {
        legacy
    };
    ctx.block().call(result, name, &operands)
}

pub(crate) fn finish(llmod: &mut LlModule) {
    if !enabled() {
        return;
    }
    MODULE.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            for line in std::mem::take(&mut m.raw) {
                llmod.add_raw_global(line);
            }
        }
    });
}

pub(crate) fn bodies(view: &crate::codegen::ProgramFactsView) {
    if enabled() {
        MODULE.with(|m| {
            let mut m = m.borrow_mut();
            let m = m.as_mut().expect("verification module initialized");
            if let Some(path) = &view.verification_module {
                m.name = path.clone();
            }
        });
    }
    if !enabled() && !dce_enabled() {
        return;
    }
    let name = view.verification_module.clone().unwrap_or_default();
    BODIES.with(|b| {
        *b.borrow_mut() = view.verification_bodies.clone().map(|map| (name, map));
    });
}

/// The body is classified and no path reaches it. An absent map (give-up,
/// incomplete reach) or an unknown identity is active.
fn inactive(id: u32, name: &str) -> bool {
    BODIES.with(|b| {
        let b = b.borrow();
        let Some((module, bodies)) = b.as_ref() else {
            return false;
        };
        let name = if id == u32::MAX {
            format!("{module}:{name}")
        } else {
            String::new()
        };
        bodies.get(&(id, name)) == Some(&false)
    })
}

/// Removal: an inactive body's entry traps. `llvm.trap` is noreturn, so the
/// optimizer drops the statements lowered after it.
fn trap(block: &mut crate::block::LlBlock) {
    block.call_void("llvm.trap", &[]);
}

pub(crate) fn body_entry(ctx: &mut FnCtx<'_>, id: u32, name: &str) {
    if (!enabled() && !dce_enabled()) || !inactive(id, name) {
        return;
    }
    if !enabled() {
        trap(ctx.block());
        return;
    }
    MODULE.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.copy = ctx.func.name.clone();
        }
    });
    coverage::entry(id, name, &ctx.func.name);
    let site = raw_site(
        ctx,
        "Activation",
        Some("<entry>"),
        &[],
        "known inactive body",
    );
    ctx.block().call_void(
        "perry_verify_facts_entry",
        &[(I64, &site.pointer), (I64, site.len.as_deref().unwrap())],
    );
    if dce_enabled() {
        trap(ctx.block());
    }
}

pub(crate) fn raw_body_entry(lf: &mut crate::function::LlFunction, id: u32) {
    if !enabled() {
        if dce_enabled() && inactive(id, "") {
            trap(lf.block_mut(0).unwrap());
        }
        return;
    }
    MODULE.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.copy = lf.name.clone();
        }
    });
    if !inactive(id, "") {
        return;
    }
    coverage::entry(id, "", &lf.name);
    let (module, ordinal) = MODULE.with(|m| {
        let m = m.borrow();
        let m = m.as_ref().unwrap();
        (m.name.clone(), m.ordinal)
    });
    let meta = serde_json::json!({"marker":"PERRY_FALSE_PROOF", "module":module,
        "byte_offset":null, "key":"<entry>", "verdict":"Activation", "proven_receivers":[],
        "copy":{"body":lf.name,"emission":ordinal}, "compile_sha":option_env!("PERRY_BUILD_GIT_SHA").unwrap_or("unknown")});
    let mut bytes = serde_json::to_vec(&meta).unwrap();
    bytes.pop();
    let (ptr, len) = raw_bytes("__perry_body", &bytes);
    lf.block_mut(0)
        .unwrap()
        .call_void("perry_verify_facts_entry", &[(I64, &ptr), (I64, &len)]);
    if dce_enabled() {
        trap(lf.block_mut(0).unwrap());
    }
}

pub(crate) fn field_consumed(p: &crate::expr::wpfacts_access::FieldProof, key: &str) {
    coverage::field(p, key, false);
}
pub(crate) fn field_checked(p: &crate::expr::wpfacts_access::FieldProof, key: &str) {
    coverage::field(p, key, true);
}

fn source_key(e: &Expr) -> String {
    match e {
        Expr::PropertyGet { property, .. }
        | Expr::PropertySet { property, .. }
        | Expr::PropertyUpdate { property, .. } => property.clone(),
        Expr::Call { callee, .. } => source_key(callee),
        Expr::New { .. } => "<new>".into(),
        _ => "<value>".into(),
    }
}

pub(crate) fn number_checked(id: u32) {
    coverage::number(id);
}

pub(crate) fn int32_checked(id: u32) {
    coverage::int32(id);
}

pub(crate) fn const_length_checked(id: u32) {
    coverage::const_length(id);
}

pub(crate) fn covered(fact: Fact, operands: impl FnOnce() -> serde_json::Value) {
    if enabled() {
        coverage::checked(fact, operands());
    }
}

pub(crate) fn bodies_checked(bodies: &[crate::codegen::FactsBody], width: usize) {
    covered(
        Fact::Bodies,
        || serde_json::json!({"bodies":bodies.iter().map(|b| (b.func_id,b.params,b.closure)).collect::<Vec<_>>(),"call_width":width}),
    );
}

pub(crate) fn normal_call_miss(class: &str, key: &str) {
    coverage::normal_call_miss(class, key);
}

pub fn reset() {
    if enabled() {
        coverage::reset();
    }
}

pub fn write_manifest(path: &std::path::Path) -> std::io::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let path = std::path::PathBuf::from(format!("{}.facts.jsonl", path.display()));
    std::fs::write(path, take_manifest().join("\n") + "\n")
}

pub(crate) fn tick(ctx: &mut FnCtx<'_>) {
    if enabled() {
        ctx.block().call_void("perry_verify_facts_tick", &[]);
    }
}

//! `PERRY_VERIFY_FACTS=1` while compiling: the facts-verification build.
//!
//! Every lowering that codegen takes from a whole-program fact (a field
//! access elided to its birth slot, a call of the proven bodies, a method
//! call switched over the proven receiver classes, a static call without its
//! guard, a dispatch-tower case without its layout recheck) first checks at
//! run time what the fact claimed of the live value: the receiver's class id
//! is one of the proven classes and the key sits at the proven slot, the
//! callee is one of the proven bodies, the receiver still has its birth
//! layout. A value the fact did not admit calls one runtime witness
//! (`perry-runtime` `wpfacts_check.rs`), which prints the site
//! (`file:line:col`, the lowering, what was expected) and what it found, and
//! aborts. It is a soundness oracle for wpfacts: a run that aborts found a
//! false proof.
//!
//! The witnesses are the contract build's (`PERRY_CHECK_CONTRACTS=1` emits
//! them too); this switch emits only them, links the ordinary runtime and
//! keeps every other lowering. The variable is read while compiling, never
//! by the program, and is part of the build and object cache keys. Unset,
//! codegen emits exactly what it emits without this module: nothing here
//! adds code, a string or a declaration.
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I64};

/// The facts-verification build was asked for.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_VERIFY_FACTS").as_deref() == Ok("1"))
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
}

thread_local! {
    static MODULE: RefCell<Option<Module>> = const { RefCell::new(None) };
    /// The innermost source site being lowered: SWC 1-based start, kind.
    static SITE: Cell<Option<(u32, &'static str)>> = const { Cell::new(None) };
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
        })
    });
    SITE.with(|s| s.set(None));
}

pub(crate) struct SiteGuard {
    outer: Option<(u32, &'static str)>,
}

/// Lowering `e` starts: a source site (a call, read, write) becomes the
/// innermost site until the guard drops.
pub(crate) fn enter(e: &Expr) -> Option<SiteGuard> {
    if !witnesses() {
        return None;
    }
    let here = crate::facts_consumed::site_of(e).map(|(s, _, k)| (s, k));
    let outer = SITE.with(|s| s.get());
    if here.is_some() {
        SITE.with(|s| s.set(here));
    }
    Some(SiteGuard { outer })
}

impl Drop for SiteGuard {
    fn drop(&mut self) {
        SITE.with(|s| s.set(self.outer));
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
pub(crate) fn site_arg(ctx: &mut FnCtx<'_>, what: &str) -> String {
    let text = format!("{} {what}", here());
    let idx = ctx.strings.intern(&text);
    let global = format!("@{}", ctx.strings.entry(idx).handle_global);
    let blk = ctx.block();
    let boxed = blk.load(DOUBLE, &global);
    let bits = blk.bitcast_double_to_i64(&boxed);
    blk.and(I64, &bits, POINTER_MASK_I64)
}

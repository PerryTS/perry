//! Dump only: which source sites codegen read a program fact for.
//!
//! With `PERRY_FACTS_CONSUMED=<file>` set, the lookups that read the facts
//! (`ElisionFacts`: a binding's receiver classes, a field, the bodies a local
//! holds, a local's class) call [`used`]; the guard [`enter`] returns, one per
//! lowered expression, attributes the lookups made while lowering that
//! expression itself (not its operands) to its source site, and the driver
//! writes the set beside the facts dump. Nothing is recorded, and nothing in
//! the emitted code changes, without the variable.
use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use perry_hir::Expr;

/// (module name, SWC 1-based start, end, kind)
type Mark = (String, u32, u32, &'static str);

static MARKS: Mutex<BTreeSet<Mark>> = Mutex::new(BTreeSet::new());
static MODULE: Mutex<()> = Mutex::new(());

thread_local! {
    static USED: Cell<bool> = const { Cell::new(false) };
    static MODULE_NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_FACTS_CONSUMED").is_some_and(|v| !v.is_empty()))
}

/// A lookup returned a fact that drives the lowering of the current expression.
pub(crate) fn used() {
    if enabled() {
        USED.with(|u| u.set(true));
    }
}

/// The module being lowered (its HIR name).
pub(crate) fn begin_module(name: &str) {
    if enabled() {
        let _ = &MODULE;
        MODULE_NAME.with(|m| *m.borrow_mut() = name.to_owned());
    }
}

pub(crate) struct Guard {
    outer: bool,
    site: Option<(u32, u32, &'static str)>,
    /// The facts-verification build's innermost site (`verify_facts`).
    _verify: Option<crate::verify_facts::SiteGuard>,
}

pub(crate) fn site_of(e: &Expr) -> Option<(u32, u32, &'static str)> {
    let (s, z, k) = match e {
        Expr::Call {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "call"),
        Expr::New {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "new"),
        Expr::PropertyGet {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "property"),
        Expr::IndexGet {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "element"),
        Expr::PropertySet {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "write"),
        Expr::PropertyUpdate {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "update"),
        Expr::IndexSet {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end, "element-write"),
        _ => return None,
    };
    (s != 0).then_some((s, z, k))
}

/// Starts the expression's own lookups; ends them when dropped.
pub(crate) fn enter(e: &Expr) -> Option<Guard> {
    let verify = crate::verify_facts::enter(e);
    if !enabled() && verify.is_none() {
        return None;
    }
    Some(Guard {
        outer: USED.with(|u| u.replace(false)),
        site: site_of(e),
        _verify: verify,
    })
}

impl Drop for Guard {
    fn drop(&mut self) {
        let used = USED.with(|u| u.replace(self.outer));
        if let (true, Some((s, z, k))) = (used, self.site) {
            let m = MODULE_NAME.with(|m| m.borrow().clone());
            MARKS.lock().expect("marks poisoned").insert((m, s, z, k));
        }
    }
}

/// The sites marked so far, as `module \t start \t end \t kind` lines.
pub fn take_lines() -> Vec<String> {
    std::mem::take(&mut *MARKS.lock().expect("marks poisoned"))
        .into_iter()
        .map(|(m, s, z, k)| format!("{m}\t{s}\t{z}\t{k}"))
        .collect()
}

//! Opt-in, compiler-only generic fallback evidence. Nothing is emitted when
//! disabled. LLVM pseudo-probes carry labels to native PCs without executing
//! instructions; the driver moves their sections into `<exe>.fallback.json`.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use perry_hir::Expr;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receiver {
    pub classes: Vec<u32>,
    pub primitive_mask: u16,
    pub other: bool,
    pub top: bool,
    pub cause: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub module_path: String,
    pub analysis_refusal: Option<String>,
    pub locals: BTreeMap<u32, Receiver>,
    pub this_of: BTreeMap<u32, Receiver>,
    pub sites: BTreeMap<(u32, String, bool), (Receiver, String)>,
    pub class_refusals: BTreeMap<u32, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallLabel {
    pub function: String,
    pub index: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Record {
    pub guid: u64,
    pub index: u64,
    pub labels: Vec<CallLabel>,
    pub module: String,
    pub symbol: String,
    pub block: String,
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub byte_offset: u32,
    pub approximate: bool,
    pub key: String,
    pub operation: String,
    pub receiver: Receiver,
    pub guard: String,
    pub reason: String,
    pub proof_refusals: Vec<String>,
    pub facts_verdict: Option<String>,
    pub analysis_refusal: Option<String>,
    pub receiver_refusals: Vec<String>,
    pub helper: String,
    pub compiler_file: String,
    pub compiler_line: u32,
    pub compiler_column: u32,
}

#[derive(Clone, Default)]
struct Context {
    expr_identity: usize,
    offset: u32,
    approximate: bool,
    key: String,
    operation: String,
    receiver: Receiver,
    reason: String,
    guard: String,
    refusals: Vec<String>,
    facts_verdict: Option<String>,
    receiver_refusals: Vec<String>,
}

struct Session {
    guid: u64,
    module: String,
    source: String,
    line_starts: Vec<usize>,
    prefix_lines: u32,
    facts: Arc<Facts>,
    contexts: Vec<Context>,
    symbol: String,
    records: Vec<Record>,
    callee_cause: Option<String>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}
static CALL_LABELS: Mutex<BTreeMap<(u64, u64), Vec<CallLabel>>> = Mutex::new(BTreeMap::new());
static RECORDS: Mutex<Vec<Record>> = Mutex::new(Vec::new());
static FAMILY_REFUSALS: Mutex<BTreeMap<u32, String>> = Mutex::new(BTreeMap::new());

pub(crate) fn family_refused(id: u32, why: &str) {
    if enabled() {
        FAMILY_REFUSALS
            .lock()
            .expect("family refusals poisoned")
            .insert(id, why.into());
    }
}

pub fn reset() {
    CALL_LABELS
        .lock()
        .expect("fallback labels poisoned")
        .clear();
    RECORDS.lock().expect("fallback records poisoned").clear();
    FAMILY_REFUSALS
        .lock()
        .expect("family refusals poisoned")
        .clear();
}

pub fn enabled() -> bool {
    std::env::var("PERRY_FALLBACK_DIAG").as_deref() == Ok("1")
}

pub struct ModuleGuard;
pub(crate) fn begin(
    module: &str,
    source: Option<&str>,
    prefix_lines: u32,
    facts: Arc<Facts>,
) -> Option<ModuleGuard> {
    if !enabled() {
        return None;
    }
    let mut guid = 0xcbf29ce484222325u64;
    for b in module.bytes() {
        guid = (guid ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let module = if facts.module_path.is_empty() {
        module.to_owned()
    } else {
        facts.module_path.clone()
    };
    let source = source.unwrap_or("").to_owned();
    let line_starts = std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
        )
        .collect();
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session {
            guid,
            module,
            source,
            line_starts,
            prefix_lines,
            facts,
            contexts: Vec::new(),
            symbol: String::new(),
            records: Vec::new(),
            callee_cause: None,
        })
    });
    Some(ModuleGuard)
}

impl Drop for ModuleGuard {
    fn drop(&mut self) {
        SESSION.with(|s| {
            if let Some(s) = s.borrow_mut().take() {
                RECORDS
                    .lock()
                    .expect("fallback records poisoned")
                    .extend(s.records);
            }
        });
    }
}

pub fn take_records() -> Vec<Record> {
    let mut records = std::mem::take(&mut *RECORDS.lock().expect("fallback records poisoned"));
    let labels = CALL_LABELS.lock().expect("fallback labels poisoned");
    for record in &mut records {
        record.labels = labels
            .get(&(record.guid, record.index))
            .cloned()
            .unwrap_or_default();
    }
    records
}

pub(crate) fn module_probe() -> Option<(u64, String)> {
    SESSION.with(|s| {
        s.borrow()
            .as_ref()
            .filter(|s| !s.records.is_empty())
            .map(|s| (s.guid, s.module.clone()))
    })
}

pub(crate) fn set_symbol(symbol: &str) {
    SESSION.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            if s.symbol != symbol {
                s.symbol = symbol.to_owned();
            }
        }
    });
}

pub(crate) struct ExprGuard;
impl Drop for ExprGuard {
    fn drop(&mut self) {
        SESSION.with(|s| {
            if let Some(s) = s.borrow_mut().as_mut() {
                s.contexts.pop();
            }
        });
    }
}

pub(crate) fn enter_expr(ctx: &crate::expr::FnCtx<'_>, expr: &Expr) -> Option<ExprGuard> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let s = slot.as_mut()?;
        let expr_identity = expr as *const Expr as usize;
        if s.contexts
            .last()
            .is_some_and(|c| c.expr_identity == expr_identity)
        {
            return None;
        }
        let previous = s.contexts.last().cloned().unwrap_or_default();
        let mut c = Context {
            expr_identity,
            offset: previous.offset,
            approximate: true,
            ..Default::default()
        };
        let (receiver, key, operation, offset) = match expr {
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                ..
            } => (
                Some(object.as_ref()),
                property.as_str(),
                "get",
                *byte_offset,
            ),
            Expr::PropertySet {
                object, property, ..
            } => (Some(object.as_ref()), property.as_str(), "store", 0),
            Expr::PropertyUpdate {
                object, property, ..
            } => (Some(object.as_ref()), property.as_str(), "update", 0),
            Expr::PutValueSet { target, key, .. } | Expr::ReflectSet { target, key, .. } => (
                Some(target.as_ref()),
                match key.as_ref() {
                    Expr::String(s) => s.as_str(),
                    _ => "computed",
                },
                if matches!(key.as_ref(), Expr::String(_)) {
                    "store"
                } else {
                    "computed store"
                },
                0,
            ),
            Expr::ReflectGet { target, key, .. } => (
                Some(target.as_ref()),
                match key.as_ref() {
                    Expr::String(s) => s.as_str(),
                    _ => "computed",
                },
                if matches!(key.as_ref(), Expr::String(_)) {
                    "get"
                } else {
                    "computed get"
                },
                0,
            ),
            Expr::ReflectHas { target, key } => (
                Some(target.as_ref()),
                match key.as_ref() {
                    Expr::String(s) => s.as_str(),
                    _ => "computed",
                },
                "has",
                0,
            ),
            Expr::IndexGet { object, .. } => (Some(object.as_ref()), "computed", "computed get", 0),
            Expr::IndexSet { object, .. } => {
                (Some(object.as_ref()), "computed", "computed store", 0)
            }
            Expr::IndexUpdate { object, .. } => {
                (Some(object.as_ref()), "computed", "computed update", 0)
            }
            Expr::NativeMethodCall {
                object: Some(object),
                method,
                ..
            } => (Some(object.as_ref()), method.as_str(), "method call", 0),
            Expr::Call {
                callee,
                byte_offset,
                ..
            } => match callee.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } => (
                    Some(object.as_ref()),
                    property.as_str(),
                    "method call",
                    *byte_offset,
                ),
                _ => (Some(callee.as_ref()), "computed", "call", *byte_offset),
            },
            Expr::In { object, property } => (
                Some(object.as_ref()),
                match property.as_ref() {
                    Expr::String(s) => s.as_str(),
                    _ => "computed",
                },
                "has",
                0,
            ),
            Expr::New { byte_offset, .. }
            | Expr::NewDynamic { byte_offset, .. }
            | Expr::NewDynamicSpread { byte_offset, .. } => {
                (None, "computed", "birth", *byte_offset)
            }
            _ => (None, "computed", "generated operation", 0),
        };
        c.key = key.to_owned();
        c.operation = operation.to_owned();
        if offset != 0 {
            c.offset = offset;
            c.approximate = false;
        }
        if let Some(receiver) = receiver {
            let binding = match receiver {
                Expr::LocalGet(id) => s.facts.locals.get(id),
                Expr::This => ctx
                    .this_stack
                    .last()
                    .and_then(|top| ctx.wpfacts_this.iter().rev().find(|(slot, _)| slot == top))
                    .and_then(|(_, fid)| s.facts.this_of.get(fid)),
                _ => None,
            };
            let had_binding = binding.is_some();
            c.receiver = binding.cloned().unwrap_or_else(|| Receiver {
                other: true,
                cause: Some("receiver expression has no binding fact".into()),
                ..Default::default()
            });
            if !had_binding {
                let dbg = format!("{receiver:?}");
                let end = dbg.find([' ', '(', '{']).unwrap_or(dbg.len());
                c.receiver_refusals
                    .push(format!("receiver expression kind: {}", &dbg[..end]));
            }
            for cid in &c.receiver.classes {
                if let Some(reason) = s.facts.class_refusals.get(cid) {
                    c.receiver_refusals.push(reason.clone());
                }
                if let Some(reason) = FAMILY_REFUSALS
                    .lock()
                    .expect("family refusals poisoned")
                    .get(cid)
                {
                    c.receiver_refusals.push(reason.clone());
                }
            }
        }
        if let Some((recv, reason)) = (offset != 0 && matches!(operation, "get" | "method call"))
            .then(|| {
                s.facts
                    .sites
                    .get(&(offset, key.to_owned(), operation == "method call"))
            })
            .flatten()
        {
            c.receiver.top |= recv.top;
            if recv.cause.is_some() {
                c.receiver.cause = recv.cause.clone();
            }
            c.reason = reason.clone();
            if !reason.starts_with("whole-program ") {
                c.facts_verdict = Some(reason.clone());
            }
        }
        if ctx.wpfacts_off {
            c.guard = "whole-program receiver/class/interpreted-prototype guard".into();
            c.reason = "generic miss arm of guarded whole-program field proof".into();
        }
        s.contexts.push(c);
        Some(ExprGuard)
    })
}

/// An emitter with an explicit synthetic operation or birth admission knows
/// its deciding predicate even when no HIR expression owns this helper.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emission(
    function: &str,
    key: &str,
    operation: &str,
    reason: &str,
    guard: &str,
    classes: &[u32],
) -> Option<ExprGuard> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let s = slot.as_mut()?;
        let mut c = s.contexts.last().cloned().unwrap_or_default();
        c.expr_identity = 0;
        c.key = key.into();
        c.operation = operation.into();
        c.reason = reason.into();
        c.guard = guard.into();
        c.facts_verdict = None;
        c.refusals.clear();
        c.receiver_refusals.clear();
        c.receiver = Receiver {
            classes: classes.to_vec(),
            other: classes.is_empty(),
            cause: classes
                .is_empty()
                .then(|| "generated receiver proof is not carried to this emitter".into()),
            ..Default::default()
        };
        for cid in classes {
            if let Some(reason) = s.facts.class_refusals.get(cid) {
                c.receiver_refusals.push(reason.clone());
            }
            if let Some(reason) = FAMILY_REFUSALS
                .lock()
                .expect("family refusals poisoned")
                .get(cid)
            {
                c.receiver_refusals.push(reason.clone());
            }
        }
        c.approximate |= c.offset == 0;
        s.symbol = function.into();
        s.contexts.push(c);
        Some(ExprGuard)
    })
}

/// A rejected predicate at the actual proof decision. It annotates only the
/// innermost expression; nested arguments cannot overwrite the parent's cause.
pub(crate) fn refuse(reason: &str) {
    SESSION.with(|cell| {
        let mut session = cell.borrow_mut();
        let Some(session) = session.as_mut() else {
            return;
        };
        let reason = if reason == "receiver has no prefix-stable binding proof" {
            session.facts.analysis_refusal.as_ref().map_or_else(
                || reason.to_owned(),
                |cause| format!("receiver binding proof unavailable: {cause}"),
            )
        } else {
            reason.to_owned()
        };
        if let Some(c) = session.contexts.last_mut() {
            if !c.refusals.contains(&reason) {
                c.refusals.push(reason);
            }
        }
    });
}

/// Why a call's callee value reaches `js_closure_call*`: the shape of the callee
/// and which binding fact it lacks. Scoped around the call's lowering; recorded
/// as a proof refusal on the closure-call records it emits.
pub(crate) struct CalleeGuard(Option<Option<String>>);
pub(crate) fn callee_cause(ctx: &crate::expr::FnCtx<'_>, callee: &Expr) -> CalleeGuard {
    if !enabled() {
        return CalleeGuard(None);
    }
    let cause = match callee {
        Expr::LocalGet(id) => {
            let place = if ctx.module_globals.contains_key(id) {
                "module-global"
            } else if ctx.closure_captures.contains_key(id) {
                "captured"
            } else {
                "plain"
            };
            let proof = if ctx.guard_free_closure_bindings.contains(id) {
                "single-binding guard-free"
            } else if ctx.local_closure_func_ids.contains_key(id) {
                "closure entry known, identity-guarded"
            } else if ctx.local_func_ref_ids.contains_key(id) {
                "function alias"
            } else if ctx
                .resolved_plain_callback_targets
                .keys()
                .any(|(local, _)| local == id)
            {
                "entry-resolved"
            } else {
                "no single-binding closure proof"
            };
            format!("callee local ({place}): {proof}")
        }
        other => {
            let dbg = format!("{other:?}");
            let end = dbg.find([' ', '(', '{']).unwrap_or(dbg.len());
            format!("callee expression: {}", &dbg[..end])
        }
    };
    let old = SESSION.with(|s| {
        s.borrow_mut()
            .as_mut()
            .map(|s| s.callee_cause.replace(cause))
    });
    CalleeGuard(old)
}
impl Drop for CalleeGuard {
    fn drop(&mut self) {
        if let Some(old) = self.0.take() {
            SESSION.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.callee_cause = old;
                }
            });
        }
    }
}

/// Explicitly selected generic edge, scoped around its emission.
pub(crate) struct DecisionGuard(Option<(String, String)>);
pub(crate) fn decision(reason: &str, guard: &str) -> DecisionGuard {
    let old = SESSION.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|s| s.contexts.last_mut())
            .map(|c| {
                let old = (c.reason.clone(), c.guard.clone());
                c.reason = reason.into();
                c.guard = guard.into();
                old
            })
    });
    DecisionGuard(old)
}
impl Drop for DecisionGuard {
    fn drop(&mut self) {
        if let Some((reason, guard)) = self.0.take() {
            SESSION.with(|s| {
                if let Some(c) = s.borrow_mut().as_mut().and_then(|s| s.contexts.last_mut()) {
                    c.reason = reason;
                    c.guard = guard;
                }
            });
        }
    }
}

fn generic_helper(helper: &str) -> bool {
    helper.contains("get_field_by_name")
        || helper.contains("set_field_by_name")
        || helper.contains("object_get_field")
        || helper == "js_global_get_field_slow"
        || helper.contains("object_set_field_ic")
        || helper.starts_with("perry_probe_")
        || helper.starts_with("js_dyn_index_")
        || helper.starts_with("js_object_get_index_")
        || helper.starts_with("js_object_set_index_")
        || helper.starts_with("js_typed_feedback_object_set_index_")
        || helper.starts_with("js_typed_feedback_array_index_")
        || helper.starts_with("js_typed_feedback_numeric_array_index_")
        || helper.starts_with("js_typed_feedback_plain_array_index_")
        || helper.starts_with("js_typed_array_index_")
        || matches!(
            helper,
            "js_array_get_index_or_string"
                | "js_array_set_index_or_string"
                | "js_typed_feedback_array_set_index_or_string"
                | "js_arguments_bundle_index_get"
        )
        || helper.starts_with("js_put_value_")
        || helper == "js_super_put_value_set"
        || helper == "js_object_super_put_value_set"
        || helper.starts_with("js_bm_call")
        || helper.starts_with("js_closure_call")
        || helper.starts_with("js_closure_unbox_callee_checked")
        || helper.starts_with("js_native_call_method")
        || helper.starts_with("js_class_method_entry_")
        || helper.starts_with("js_static_method_entry_")
        || helper.starts_with("js_typed_feedback_native_call_method")
        || helper.starts_with("js_super_method_call")
        || matches!(
            helper,
            "js_call_method"
                | "js_class_static_method_call"
                | "js_private_method_call"
                | "js_arguments_call_spread"
        )
        || helper.starts_with("js_method_")
        || helper.starts_with("js_object_has")
        || helper == "js_borrowed_object_has_own"
        || helper == "js_in_operator"
        || helper == "js_has_property"
        || matches!(
            helper,
            "js_reflect_get" | "js_reflect_set" | "js_reflect_has"
        )
        || helper.starts_with("js_packed_arraylike_index_")
        || helper.contains("dynamic_prop")
        || matches!(
            helper,
            "js_object_get_field_nonptr" | "js_object_get_field_slow" | "js_object_get_field"
        )
        || helper.starts_with("js_object_set_keyed")
        || helper == "js_object_get_keyed_slow"
        || helper == "js_class_field_set_fallback"
        || helper.starts_with("js_object_set_field")
        || helper.starts_with("js_class_field_add")
        || helper.starts_with("js_object_alloc_with_parent")
        || helper.starts_with("js_object_create")
        || helper.starts_with("js_object_spread")
        || helper.starts_with("js_object_literal_define")
        || matches!(
            helper,
            "perry_alloc_class_family"
                | "js_new_function_construct_site"
                | "js_class_value_super_construct_site"
        )
        || helper.starts_with("perry_build_runtime_record")
        || helper.starts_with("perry_create_family")
}

#[track_caller]
pub(crate) fn record(block: &str, helper: &str) -> Option<(u64, u64)> {
    if !generic_helper(helper) {
        return None;
    }
    let loc = std::panic::Location::caller();
    SESSION.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut()?;
        let mut c = s.contexts.last().cloned().unwrap_or_default();
        if helper.starts_with("js_object_has")
            || matches!(
                helper,
                "js_in_operator"
                    | "js_has_property"
                    | "js_borrowed_object_has_own"
                    | "js_reflect_has"
            )
        {
            if c.operation != "has" {
                c.key = "computed".into();
            }
            c.operation = "has".into();
            if c.reason.is_empty() {
                c.reason =
                    "property presence requires receiver layout and prototype-chain semantics"
                        .into();
            }
            c.guard = "own-layout presence or inherited property lookup".into();
        }
        if helper.starts_with("js_closure_call") || helper.starts_with("js_bm_call") {
            if let Some(cause) = &s.callee_cause {
                c.refusals.push(cause.clone());
            }
            if c.reason.is_empty() {
                c.reason = "callee value is not resolved to a direct entry on this path".into();
            }
            if c.guard.is_empty() {
                c.guard = "callable tag and entry/receiver identity".into();
            }
        }
        if c.operation.is_empty() {
            c.operation = if helper.contains("construct")
                || helper.contains("alloc")
                || helper.contains("create")
                || helper.contains("class_field_add")
            {
                "birth"
            } else if helper.contains("set") {
                "store"
            } else if helper.contains("method") || helper.starts_with("js_bm_call") {
                "method call"
            } else if helper.contains("call") {
                "call"
            } else {
                "get"
            }
            .into();
        }
        let index = s.records.len() as u64 * 2 + 1;
        let off = c.offset.saturating_sub(1) as usize;
        let (line, column) = if c.offset != 0 && off <= s.source.len() {
            let line = s.line_starts.partition_point(|&start| start <= off);
            (
                (line as u32)
                    .checked_sub(s.prefix_lines)
                    .filter(|line| *line > 0),
                Some((off - s.line_starts[line - 1] + 1) as u32),
            )
        } else {
            (None, None)
        };
        let reason = if !c.reason.is_empty() {
            c.reason.clone()
        } else if c.receiver.top {
            format!(
                "receiver from unmodeled code (TOP) <- {}",
                c.receiver.cause.as_deref().unwrap_or("unknown TOP origin")
            )
        } else if let Some(cause) = &c.receiver.cause {
            cause.clone()
        } else if let Some(reason) = c.refusals.first() {
            reason.clone()
        } else if c.key == "computed" {
            "computed key has no compile-time fixed slot".into()
        } else {
            "no binding-level fixed-slot proof at generic helper emission".into()
        };
        s.records.push(Record {
            guid: s.guid,
            index,
            labels: Vec::new(),
            module: s.module.clone(),
            symbol: s.symbol.clone(),
            block: block.into(),
            file: s.module.clone(),
            line,
            column,
            byte_offset: c.offset,
            approximate: c.approximate || c.offset == 0,
            key: c.key,
            operation: c.operation,
            receiver: c.receiver,
            guard: if c.guard.is_empty() {
                "receiver/layout/presence or prototype dispatch".into()
            } else {
                c.guard
            },
            reason,
            proof_refusals: c.refusals,
            facts_verdict: c.facts_verdict,
            analysis_refusal: s.facts.analysis_refusal.clone(),
            receiver_refusals: c.receiver_refusals,
            helper: helper.into(),
            compiler_file: loc.file().into(),
            compiler_line: loc.line(),
            compiler_column: loc.column(),
        });
        Some((s.guid, index))
    })
}

/// Inert markers survive textual rooting/EH rewrites. The final IR pass puts
/// LLVM's call-site probe discriminator on the helper call itself.
pub(crate) fn probe(guid: u64, index: u64, helper: &str) -> String {
    format!("; perryfb {guid} {index} {helper}")
}

pub(crate) fn call_label(guid: u64, index: u64, function: &str, local: u32) {
    let mut labels = CALL_LABELS.lock().expect("fallback labels poisoned");
    let entries = labels.entry((guid, index)).or_default();
    if !entries
        .iter()
        .any(|v| v.function == function && v.index == local)
    {
        entries.push(CallLabel {
            function: function.into(),
            index: local,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_presence_helper_keeps_its_operation_without_a_source_context() {
        SESSION.with(|s| {
            *s.borrow_mut() = Some(Session {
                guid: 1,
                module: "generated".into(),
                source: String::new(),
                line_starts: vec![0],
                prefix_lines: 0,
                facts: Arc::default(),
                contexts: Vec::new(),
                symbol: "wrapper".into(),
                records: Vec::new(),
                callee_cause: None,
            })
        });
        for helper in [
            "js_has_property",
            "js_borrowed_object_has_own",
            "js_reflect_has",
        ] {
            assert!(record("entry", helper).is_some());
        }
        let records = SESSION.with(|s| s.borrow_mut().take().unwrap().records);
        assert_eq!(records.len(), 3);
        assert!(records
            .iter()
            .all(|r| r.operation == "has" && r.key == "computed" && r.approximate));
    }
}

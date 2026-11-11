//! LIFE D1: the frame-placement census (compiler-internal, off by default).
//!
//! With `PERRY_LIFE_CENSUS=<path>` set at compile time, every function's
//! escape analysis (`escape_news::collect_non_escaping_news`) also classifies
//! each `let x = new C(...)` candidate, and appends one TSV row per `new` to
//! `<path>`: `module, function, class, offset, slots, verdict, detail`.
//!
//! Verdicts:
//! * `scalar`: the escape analysis dissolves it into locals today;
//! * `frame`: it meets the frame rule of the LIFE D design (slice 1);
//! * `heap:<reason>`: the FIRST reason that blocks the frame rule, checked in
//!   this order: `async-or-generator` (the creating function), then the
//!   class: `class-not-fixed-birth` (extends, decorators, computed members or
//!   accessors), `no-facts` (no whole-program receiver fact for the local),
//!   `too-many-slots` (more than 8), `pointer-or-unknown-field-kind` (a field
//!   whose whole-program kind may be a pointer or is unknown); then the uses
//!   in walk order: `call-kept:<callee kind>` (direct function, method,
//!   method-receiver, constructor, unknown target, runtime/builtin),
//!   `call-returned:direct function`, `call-bet:direct function` (dropped on a
//!   guarded proof), `returned`, `thrown`, `stored-into-object`,
//!   `stored-into-module-variable`, `aliased-to-local`, `reassigned`,
//!   `captured-by-closure`, `written-after-birth`, `read-guarded`,
//!   `read-unproven`, `unrecognised:<HIR expression kind>`;
//! * `none:<position>`: a `new` that is not a candidate at all (not bound to
//!   a plain local): `argument` (detail: the callee's fate for that position
//!   when the callee is a direct function), `return`, `array-element`,
//!   `field-initializer`, `let-boxed-or-global`, `other:<kind>`.
//!
//! The census changes nothing codegen emits: it reads the HIR and the facts
//! and writes only its own file. Parameter fates are the general rule of
//! `perry_transform::effects` (`Fates`), with [`CodegenProof`] answering its
//! read question from the same predicate `wpfacts_access::prove` gates on.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use perry_hir::types::FuncId;
use perry_hir::{CompareOp, Expr, Stmt};
use perry_transform::effects::{direct_target, Fate, FateProof, Fates, ParamFate, Proof};

use crate::codegen::ElisionFacts;

/// `FactsField::kind_mask` bits that may hold a pointer (string, object,
/// function, bigint/symbol), as `wpfacts_access::K_POINTER`.
const K_POINTER: u16 = 2 | 32 | 64 | 128;
const MAX_SLOTS: usize = 8;

fn path() -> Option<&'static str> {
    static P: OnceLock<Option<String>> = OnceLock::new();
    P.get_or_init(|| {
        std::env::var("PERRY_LIFE_CENSUS")
            .ok()
            .filter(|p| !p.is_empty())
    })
    .as_deref()
}

/// The fate questions answered from the whole-program facts: a read is
/// code-free when `wpfacts_access::facts_read` says the bare load needs no
/// guard; a guarded one is a bet on its site; call bodies are direct
/// function references only (slice 1).
pub(crate) struct CodegenProof<'a> {
    pub facts: &'a ElisionFacts,
}

impl FateProof for CodegenProof<'_> {
    fn read(&self, func: FuncId, object: &Expr, property: &str, site: u32) -> Option<Proof> {
        let recv = match object {
            Expr::LocalGet(id) => self.facts.locals.get(id)?,
            Expr::This => self.facts.this_of.get(&func)?,
            _ => return None,
        };
        match crate::expr::wpfacts_access::facts_read(self.facts, recv, property) {
            Ok(true) => Some(Proof::Proven),
            Ok(false) => Some(Proof::Bet(u64::from(site))),
            Err(_) => None,
        }
    }
    fn store(&self, _: FuncId, _: &str) -> Option<Proof> {
        None
    }
    fn targets(&self, callee: &Expr) -> Option<(Vec<FuncId>, Proof)> {
        direct_target(callee)
    }
}

struct ModuleCensus {
    name: String,
    facts: Arc<ElisionFacts>,
    fates: Fates,
    /// Body address -> (label, async or generator).
    bodies: HashMap<usize, (String, bool)>,
    classes: HashMap<String, bool>,
    seen: Mutex<HashSet<(String, u32, String)>>,
}

fn registry() -> &'static Mutex<Vec<Arc<ModuleCensus>>> {
    static R: OnceLock<Mutex<Vec<Arc<ModuleCensus>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Arc<ModuleCensus>>> = const { std::cell::RefCell::new(None) };
}

/// Ends one module's census registration.
pub(crate) struct Registered(Arc<ModuleCensus>);

impl Drop for Registered {
    fn drop(&mut self) {
        registry()
            .lock()
            .expect("census registry")
            .retain(|m| !Arc::ptr_eq(m, &self.0));
        CURRENT.with(|c| c.borrow_mut().take());
    }
}

/// Starts the census of one module (a no-op unless `PERRY_LIFE_CENSUS`).
pub(crate) fn register(hir: &perry_hir::Module, facts: &Arc<ElisionFacts>) -> Option<Registered> {
    path()?;
    let m = Arc::new(ModuleCensus::new(hir, facts));
    registry().lock().expect("census registry").push(m.clone());
    CURRENT.with(|c| *c.borrow_mut() = Some(m.clone()));
    Some(Registered(m))
}

impl ModuleCensus {
    fn new(hir: &perry_hir::Module, facts: &Arc<ElisionFacts>) -> Self {
        let fates = Fates::of_module(hir, &CodegenProof { facts });
        let mut bodies = HashMap::new();
        let mut note = |f: &perry_hir::Function, label: String| {
            bodies.insert(
                f.body.as_ptr() as usize,
                (label, f.is_async || f.is_generator),
            );
        };
        for f in &hir.functions {
            note(f, f.name.clone());
        }
        let mut classes = HashMap::new();
        for c in &hir.classes {
            if let Some(k) = &c.constructor {
                note(k, format!("{}.constructor", c.name));
            }
            for m in c.methods.iter().chain(&c.static_methods) {
                note(m, format!("{}.{}", c.name, m.name));
            }
            let plain = c.extends.is_none()
                && c.extends_name.is_none()
                && c.native_extends.is_none()
                && c.extends_expr.is_none()
                && c.decorators.is_empty()
                && c.computed_members.is_empty()
                && c.getters.is_empty()
                && c.setters.is_empty();
            classes.insert(c.name.clone(), plain);
        }
        ModuleCensus {
            name: hir.name.clone(),
            facts: facts.clone(),
            fates,
            bodies,
            classes,
            seen: Mutex::new(HashSet::new()),
        }
    }
}

/// One function's census, from the escape analysis of its body.
pub(crate) fn observe(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    scalar: &HashMap<u32, String>,
) {
    let Some(out) = path() else { return };
    let key = stmts.as_ptr() as usize;
    let found = registry()
        .lock()
        .expect("census registry")
        .iter()
        .find_map(|m| m.bodies.get(&key).map(|b| (m.clone(), b.clone())));
    let (module, (label, suspends)) = match found {
        Some(f) => f,
        None => match CURRENT.with(|c| c.borrow().clone()) {
            Some(m) => (m, ("<init>".to_string(), false)),
            None => return,
        },
    };
    let rows = classify(&module, stmts, boxed_vars, module_globals, scalar, suspends);
    let mut seen = module.seen.lock().expect("census seen");
    let mut text = String::new();
    for (class, offset, slots, verdict, detail) in rows {
        if !seen.insert((label.clone(), offset, class.clone())) {
            continue;
        }
        let slots = slots.map_or("-".to_string(), |s| s.to_string());
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            module.name, label, class, offset, slots, verdict, detail
        ));
    }
    drop(seen);
    if text.is_empty() {
        return;
    }
    static WRITE: Mutex<()> = Mutex::new(());
    let _w = WRITE.lock().expect("census write");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
    {
        let _ = f.write_all(text.as_bytes());
    }
}

type Row = (String, u32, Option<usize>, String, String);

/// The census rows of one body: every candidate's verdict, and every `new`
/// that is not a candidate.
fn classify(
    module: &ModuleCensus,
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    scalar: &HashMap<u32, String>,
    suspends: bool,
) -> Vec<Row> {
    let mut candidates = HashMap::new();
    super::find_new_candidates(stmts, boxed_vars, module_globals, &mut candidates);
    let mut c = Census {
        m: module,
        candidates: &candidates,
        globals: module_globals,
        first: HashMap::new(),
        rows: Vec::new(),
        defs: HashMap::new(),
    };
    for s in stmts {
        c.stmt(s);
    }
    let mut rows = std::mem::take(&mut c.rows);
    for (id, class) in &candidates {
        let (offset, _) = c.defs.get(id).copied().unwrap_or((0, 0));
        let slots = c.slots(*id);
        let verdict = if scalar.contains_key(id) {
            "scalar".to_string()
        } else if let Some(r) = c.class_reason(*id, class, suspends) {
            format!("heap:{r}")
        } else if let Some(r) = c.first.get(id) {
            format!("heap:{r}")
        } else {
            "frame".to_string()
        };
        rows.push((class.clone(), offset, slots, verdict, String::new()));
    }
    rows
}

struct Census<'a> {
    m: &'a ModuleCensus,
    candidates: &'a HashMap<u32, String>,
    globals: &'a HashMap<u32, String>,
    /// First use that blocks each candidate.
    first: HashMap<u32, String>,
    rows: Vec<Row>,
    /// Candidate -> (offset of its `new`, unused).
    defs: HashMap<u32, (u32, u32)>,
}

fn kind(e: &Expr) -> String {
    let text = format!("{e:?}");
    text.split(|c: char| c == '(' || c == ' ' || c == '{')
        .next()
        .unwrap_or("?")
        .to_string()
}

fn fate_word(f: ParamFate) -> &'static str {
    match (f.fate, f.proof) {
        (Fate::Dropped, Proof::Proven) => "Dropped",
        (Fate::Dropped, Proof::Bet(_)) => "Dropped(bet)",
        (Fate::Returned, _) => "Returned",
        (Fate::KeptByReceiver, _) => "KeptByReceiver",
        (Fate::Kept, _) => "Kept",
    }
}

impl Census<'_> {
    fn cand(&self, e: &Expr) -> Option<u32> {
        match e {
            Expr::LocalGet(id) if self.candidates.contains_key(id) => Some(*id),
            _ => None,
        }
    }

    fn block(&mut self, id: u32, why: impl Into<String>) {
        self.first.entry(id).or_insert_with(|| why.into());
    }

    fn class_id(&self, id: u32) -> Option<u32> {
        let r = self.m.facts.locals.get(&id)?;
        (r.class_ids.len() == 1).then(|| r.class_ids[0])
    }

    fn slots(&self, id: u32) -> Option<usize> {
        let cid = self.class_id(id)?;
        Some(self.m.facts.classes.get(&cid)?.fields.len())
    }

    fn class_reason(&self, id: u32, class: &str, suspends: bool) -> Option<&'static str> {
        if suspends {
            return Some("async-or-generator");
        }
        if !self.m.classes.get(class).copied().unwrap_or(false) {
            return Some("class-not-fixed-birth");
        }
        let Some(cid) = self.class_id(id) else {
            return Some("no-facts");
        };
        let Some(fc) = self.m.facts.classes.get(&cid) else {
            return Some("no-facts");
        };
        if !fc.fixed_layout {
            return Some("class-not-fixed-birth");
        }
        if fc.fields.len() > MAX_SLOTS {
            return Some("too-many-slots");
        }
        if fc
            .fields
            .iter()
            .any(|f| f.kind_mask == 0 || f.kind_mask & K_POINTER != 0)
        {
            return Some("pointer-or-unknown-field-kind");
        }
        None
    }

    fn stmts(&mut self, b: &[Stmt]) {
        for s in b {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => match init {
                Expr::New {
                    args, byte_offset, ..
                } if self.candidates.contains_key(id) => {
                    self.defs.insert(*id, (*byte_offset, 0));
                    for a in args {
                        self.expr(a, None);
                    }
                }
                Expr::New { .. } => self.new_site(init, "let-boxed-or-global", String::new()),
                e => match self.cand(e) {
                    Some(c) => self.block(c, "aliased-to-local"),
                    None => self.expr(e, None),
                },
            },
            Stmt::Let { init: None, .. } => {}
            Stmt::Return(Some(e)) => match self.cand(e) {
                Some(c) => self.block(c, "returned"),
                None if matches!(e, Expr::New { .. }) => self.new_site(e, "return", String::new()),
                None => self.expr(e, None),
            },
            Stmt::Throw(e) => match self.cand(e) {
                Some(c) => self.block(c, "thrown"),
                None => self.expr(e, None),
            },
            Stmt::Expr(e) => self.expr(e, None),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, None);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, None);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(s) = init {
                    self.stmt(s);
                }
                if let Some(e) = condition {
                    self.expr(e, None);
                }
                if let Some(e) = update {
                    self.expr(e, None);
                }
                self.stmts(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    self.stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, None);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t, None);
                    }
                    self.stmts(&c.body);
                }
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    /// A `new` that is not a candidate, at `position`.
    fn new_site(&mut self, e: &Expr, position: &str, detail: String) {
        if let Expr::New {
            class_name,
            byte_offset,
            args,
            ..
        } = e
        {
            self.rows.push((
                class_name.clone(),
                *byte_offset,
                None,
                format!("none:{position}"),
                detail,
            ));
            for a in args {
                self.expr(a, None);
            }
        }
    }

    /// A candidate or a `new` as argument `i` whose fate is `fate`.
    fn argument(&mut self, a: &Expr, fate: Option<ParamFate>, callee: &str) {
        if let Some(c) = self.cand(a) {
            match fate {
                Some(f) if f.proven_dropped() => {}
                Some(f) if f.fate == Fate::Dropped => self.block(c, format!("call-bet:{callee}")),
                Some(f) if f.fate == Fate::Returned => {
                    self.block(c, format!("call-returned:{callee}"))
                }
                _ => self.block(c, format!("call-kept:{callee}")),
            }
        } else if matches!(a, Expr::New { .. }) {
            let detail = fate.map_or(callee.to_string(), |f| format!("{callee}:{}", fate_word(f)));
            self.new_site(a, "argument", detail);
        } else {
            self.expr(a, None);
        }
    }

    fn expr(&mut self, e: &Expr, parent: Option<&Expr>) {
        match e {
            Expr::LocalGet(id) => {
                if self.candidates.contains_key(id) {
                    let why = match parent {
                        Some(p @ (Expr::Array(..) | Expr::ArraySpread(_) | Expr::Object(_))) => {
                            format!("stored-into-object:{}", kind(p))
                        }
                        Some(
                            p @ (Expr::ArrayPush { .. }
                            | Expr::ArrayUnshift { .. }
                            | Expr::SetAdd { .. }),
                        ) => {
                            format!("stored-into-object:{}", kind(p))
                        }
                        Some(p) if kind(p).contains("Call") || kind(p).starts_with("Native") => {
                            format!("call-kept:runtime/builtin:{}", kind(p))
                        }
                        Some(p) => format!("unrecognised:{}", kind(p)),
                        None => "unrecognised:<statement>".to_string(),
                    };
                    self.block(*id, why);
                }
            }
            Expr::New { .. } => {
                let pos = match parent {
                    Some(Expr::Array(..) | Expr::ArraySpread(_)) => "array-element".to_string(),
                    Some(Expr::Object(_) | Expr::PropertySet { .. } | Expr::IndexSet { .. }) => {
                        "field-initializer".to_string()
                    }
                    Some(p) => format!("other:{}", kind(p)),
                    None => "other:<statement>".to_string(),
                };
                self.new_site(e, &pos, String::new());
            }
            Expr::PropertyGet {
                object, property, ..
            } if self.cand(object).is_some() => {
                let c = self.cand(object).expect("candidate");
                let verdict =
                    self.m.facts.locals.get(&c).map(|r| {
                        crate::expr::wpfacts_access::facts_read(&self.m.facts, r, property)
                    });
                match verdict {
                    Some(Ok(true)) => {}
                    Some(Ok(false)) => self.block(c, "read-guarded"),
                    _ => self.block(c, "read-unproven"),
                }
            }
            Expr::Call { callee, args, .. } => {
                let (fn_id, what) = match callee.as_ref() {
                    Expr::FuncRef(f) => (Some(*f), "direct function"),
                    Expr::PropertyGet { object, .. } => {
                        match self.cand(object) {
                            Some(c) => self.block(c, "call-kept:method-receiver"),
                            None => self.expr(object, Some(callee)),
                        }
                        (None, "method")
                    }
                    other => {
                        self.expr(other, Some(e));
                        (None, "unknown target")
                    }
                };
                for (i, a) in args.iter().enumerate() {
                    let fate = fn_id.map(|f| self.m.fates.call_arg(&[f], i));
                    self.argument(a, fate, what);
                }
            }
            Expr::Compare { op, left, right } => {
                let address = match op {
                    CompareOp::Eq | CompareOp::Ne => true,
                    CompareOp::LooseEq | CompareOp::LooseNe => {
                        matches!(left.as_ref(), Expr::Null | Expr::Undefined)
                            || matches!(right.as_ref(), Expr::Null | Expr::Undefined)
                    }
                    _ => false,
                };
                for side in [left, right] {
                    if address && self.cand(side).is_some() {
                        continue;
                    }
                    self.expr(side, Some(e));
                }
            }
            Expr::PropertySet { object, value, .. } | Expr::IndexSet { object, value, .. } => {
                match self.cand(object) {
                    Some(c) => self.block(c, "written-after-birth"),
                    None => self.expr(object, Some(e)),
                }
                if let Expr::IndexSet { index, .. } = e {
                    self.expr(index, Some(e));
                }
                match self.cand(value) {
                    Some(c) => self.block(c, "stored-into-object"),
                    None => self.expr(value, Some(e)),
                }
            }
            Expr::LocalSet(target, value) => {
                if self.candidates.contains_key(target) {
                    self.block(*target, "reassigned");
                }
                match self.cand(value) {
                    Some(c) if self.globals.contains_key(target) => {
                        self.block(c, "stored-into-module-variable")
                    }
                    Some(c) => self.block(c, "aliased-to-local"),
                    None => self.expr(value, Some(e)),
                }
            }
            Expr::GlobalSet(_, value) => match self.cand(value) {
                Some(c) => self.block(c, "stored-into-module-variable"),
                None => self.expr(value, Some(e)),
            },
            Expr::Update { id, .. } => {
                if self.candidates.contains_key(id) {
                    self.block(*id, "written-after-birth");
                }
            }
            Expr::Closure { .. } => {
                let mut refs = Vec::new();
                perry_hir::analysis::collect_local_refs_expr(e, &mut refs, &mut HashSet::new());
                for id in refs {
                    if self.candidates.contains_key(&id) {
                        self.block(id, "captured-by-closure");
                    }
                }
            }
            _ => {
                let mut kids = Vec::new();
                perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
                for k in kids {
                    self.expr(k, Some(e));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "life_census_tests.rs"]
mod tests;

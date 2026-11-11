//! LIFE D1: the frame-placement census (compiler-internal, off by default).
//!
//! With `PERRY_LIFE_CENSUS=<path>` set at compile time (the driver then
//! compiles uncached, as with `PERRY_NO_CACHE=1`), every body's escape
//! analyses (`hir_facts`: news, array and object literals) also classify
//! each heap birth of the body, and append one TSV row per birth to
//! `<path>`: `module, function, class, offset, slots, verdict, detail, kind`.
//!
//! `kind`: `new`, `object` (an object literal: HIR's `new __AnonShape_<hash>`
//! keeps that class name and the news rule; a remaining `{...}` literal or
//! spread has class `{n}`, n fields), `array` (`[...]` literal or spread; class `[n]`) or `closure`
//! (class `closure#<func id>`). `offset`: a `new`'s byte offset; for the
//! others `#<ordinal>` in the body walk (they carry no source offset).
//!
//! Verdicts:
//! * `scalar`: the escape analysis dissolves it into locals today
//!   (`escape_news`, `escape_objects`, `escape_arrays`; closures never);
//! * `frame`: it meets the frame rule of the LIFE D design (slice 1): for a
//!   closure, a plain local that is only called;
//! * `heap:<reason>`: the FIRST reason that blocks the frame rule. For a
//!   `new`, checked in this order: `async-or-generator` (the creating
//!   function), then the class: `class-not-fixed-birth` (extends,
//!   decorators, computed members or accessors), `no-facts:<why>` (no
//!   whole-program receiver fact: `module`, the module has no facts at all
//!   (facts off, over the solver bound, or nothing proven); `binding`, the
//!   local has none; `class-set-empty` or `class-set-many`, its class set
//!   is not one class; `class`, no layout facts for that class), `too-many-slots` (more than
//!   8), `pointer-or-unknown-field-kind` (a field whose whole-program kind
//!   may be a pointer or is unknown). For a literal: `literal-shape` (not a
//!   scalar candidate by its shape: empty, more than 16 fields or elements,
//!   a method that uses `this`, a spread). For a closure:
//!   `async-or-generator`. Then, for all kinds, the uses in walk order:
//!   `call-kept:<callee kind>` (direct function, method, method-receiver,
//!   constructor, unknown target, runtime/builtin), `call-returned:direct
//!   function`, `call-bet:direct function` (dropped on a guarded proof),
//!   `returned`, `thrown`, `stored-into-object`,
//!   `stored-into-module-variable`, `aliased-to-local`, `reassigned`,
//!   `captured-by-closure`, `written-after-birth`, `read-guarded`,
//!   `read-unproven`, `read-computed-key`, `unrecognised:<HIR expression
//!   kind>`; a literal the escape analysis rejects with none of these is
//!   `escape-unclassified`;
//! * `none:<position>`: a birth that is not a candidate at all (not bound
//!   to a plain local): `argument` (detail: the callee's fate for that
//!   position when the callee is a direct function; `method`, `unknown
//!   target`, or `runtime/builtin:<HIR kind>`), `method-receiver`
//!   (`new C().m()`), `field-read` (`new C().x`), `indexed-read`,
//!   `constructor-argument`, `return`, `thrown`, `expression-statement`,
//!   `array-element`, `field-initializer`, `let-boxed` (a binding boxed
//!   because a closure captures it), `let-module-global` (a module-level
//!   binding), `let-chunked` (a chunked literal birth), `let-other`,
//!   `statement-other` (a statement's own expression: a condition, a
//!   discriminant), `other:<HIR parent kind>`.
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

/// Is the census on for this process? The compile driver forces a full
/// codegen when it is (`run_pipeline`: as `PERRY_NO_CACHE=1`), because an
/// object-cache or build-cache hit skips the codegen that writes the rows,
/// and an empty census must mean "no allocation", never "cached".
pub fn enabled() -> bool {
    path().is_some()
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
    seen: Mutex<HashSet<(String, String, String)>>,
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

/// The scalar-replacement results of one body, from the escape analyses
/// codegen runs (`hir_facts`): the census reports `scalar` for exactly these.
pub(crate) struct Scalar<'a> {
    pub news: &'a HashMap<u32, String>,
    pub objects: &'a HashMap<u32, Vec<String>>,
    pub arrays: &'a HashMap<u32, u32>,
}

/// One function's census, from the escape analyses of its body.
pub(crate) fn observe(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    scalar: &Scalar<'_>,
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
    for (kind, class, offset, slots, verdict, detail) in rows {
        let offset = match kind {
            Kind::New => offset.to_string(),
            _ => format!("#{offset}"),
        };
        if !seen.insert((label.clone(), offset.clone(), class.clone())) {
            continue;
        }
        let slots = slots.map_or("-".to_string(), |s| s.to_string());
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            module.name,
            label,
            class,
            offset,
            slots,
            verdict,
            detail,
            kind.word()
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

/// The heap birth a row counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    New,
    Object,
    Array,
    Closure,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::New => "new",
            Kind::Object => "object",
            Kind::Array => "array",
            Kind::Closure => "closure",
        }
    }
}

/// The kind and the class column of a birth expression.
fn birth(e: &Expr) -> Option<(Kind, String)> {
    Some(match e {
        Expr::New { class_name, .. } => (new_kind(class_name), class_name.clone()),
        Expr::Object(props) => (Kind::Object, format!("{{{}}}", props.len())),
        Expr::ObjectSpread { .. } => (Kind::Object, "{...}".to_string()),
        Expr::Array(elements, _) => (Kind::Array, format!("[{}]", elements.len())),
        Expr::ArraySpread(_) => (Kind::Array, "[...]".to_string()),
        Expr::Closure { func_id, .. } => (Kind::Closure, format!("closure#{func_id}")),
        _ => return None,
    })
}

/// HIR lowers an object literal to `new __AnonShape_<hash>(values...)`, the
/// shape-class prefix every HIR consumer reads (`dynamic_import`,
/// `fn_infer`): that is an object literal, not a class construction.
fn new_kind(class_name: &str) -> Kind {
    if class_name.starts_with("__AnonShape_") {
        Kind::Object
    } else {
        Kind::New
    }
}

/// Where a birth that is not a candidate sits, from its parent expression.
fn position(parent: Option<&Expr>) -> String {
    match parent {
        Some(Expr::Array(..) | Expr::ArraySpread(_)) => "array-element".to_string(),
        Some(
            Expr::Object(_)
            | Expr::ObjectSpread { .. }
            | Expr::PropertySet { .. }
            | Expr::IndexSet { .. },
        ) => "field-initializer".to_string(),
        Some(Expr::New { .. }) => "constructor-argument".to_string(),
        Some(Expr::PropertyGet { .. }) => "field-read".to_string(),
        Some(Expr::IndexGet { .. }) => "indexed-read".to_string(),
        Some(p) => format!("other:{}", kind(p)),
        None => "statement-other".to_string(),
    }
}

/// kind, class, offset (a `new`'s byte offset, else its ordinal in the
/// body walk), slots, verdict, detail.
type Row = (Kind, String, u32, Option<usize>, String, String);

/// A birth bound to a plain local.
struct Cand {
    kind: Kind,
    class: String,
    /// Fields, elements or captures, for literals and closures.
    slots: Option<usize>,
    /// An object literal's keys.
    keys: Vec<String>,
    /// A `new` (of a class or of an object literal's shape class): the
    /// news rule (facts, class) applies.
    new: bool,
}

/// The census rows of one body: every candidate's verdict, and every birth
/// that is not a candidate.
fn classify(
    module: &ModuleCensus,
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    scalar: &Scalar<'_>,
    suspends: bool,
) -> Vec<Row> {
    let mut news = HashMap::new();
    super::find_new_candidates(stmts, boxed_vars, module_globals, &mut news);
    let mut objects = HashMap::new();
    super::escape_objects::find_object_literal_candidates(
        stmts,
        boxed_vars,
        module_globals,
        &mut objects,
    );
    let mut arrays = HashMap::new();
    super::escape_arrays::find_array_candidates(stmts, boxed_vars, module_globals, &mut arrays);
    let candidates = news
        .iter()
        .map(|(id, class)| {
            let cand = Cand {
                kind: new_kind(class),
                class: class.clone(),
                slots: None,
                keys: Vec::new(),
                new: true,
            };
            (*id, cand)
        })
        .collect();
    let mut c = Census {
        m: module,
        candidates,
        boxed: boxed_vars,
        globals: module_globals,
        first: HashMap::new(),
        rows: Vec::new(),
        defs: HashMap::new(),
        ordinal: 0,
    };
    for s in stmts {
        c.stmt(s);
    }
    let mut rows = std::mem::take(&mut c.rows);
    for (id, cand) in &c.candidates {
        let offset = c.defs.get(id).copied().unwrap_or(0);
        let blocked = c.first.get(id).map(|r| format!("heap:{r}"));
        let (slots, verdict) = match cand.kind {
            _ if cand.new => {
                let verdict = if scalar.news.contains_key(id) {
                    "scalar".to_string()
                } else if let Some(r) = c.class_reason(*id, &cand.class, suspends) {
                    format!("heap:{r}")
                } else {
                    blocked.unwrap_or_else(|| "frame".to_string())
                };
                (c.slots(*id), verdict)
            }
            Kind::New | Kind::Object | Kind::Array => {
                let (is_scalar, shaped) = if cand.kind == Kind::Object {
                    (scalar.objects.contains_key(id), objects.contains_key(id))
                } else {
                    (scalar.arrays.contains_key(id), arrays.contains_key(id))
                };
                let verdict = if is_scalar {
                    "scalar".to_string()
                } else if !shaped {
                    "heap:literal-shape".to_string()
                } else {
                    blocked.unwrap_or_else(|| "heap:escape-unclassified".to_string())
                };
                (cand.slots, verdict)
            }
            Kind::Closure => (
                cand.slots,
                if suspends {
                    "heap:async-or-generator".to_string()
                } else {
                    blocked.unwrap_or_else(|| "frame".to_string())
                },
            ),
        };
        rows.push((
            cand.kind,
            cand.class.clone(),
            offset,
            slots,
            verdict,
            String::new(),
        ));
    }
    rows
}

struct Census<'a> {
    m: &'a ModuleCensus,
    candidates: HashMap<u32, Cand>,
    boxed: &'a HashSet<u32>,
    globals: &'a HashMap<u32, String>,
    /// First use that blocks each candidate.
    first: HashMap<u32, String>,
    rows: Vec<Row>,
    /// Candidate -> offset of its birth (byte offset or ordinal, as rows).
    defs: HashMap<u32, u32>,
    /// Next ordinal of a literal or closure birth in this body.
    ordinal: u32,
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

    fn cand_kind(&self, id: u32) -> Kind {
        self.candidates[&id].kind
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
        let facts = &self.m.facts;
        let Some(recv) = facts.locals.get(&id) else {
            return Some(if facts.locals.is_empty() && facts.classes.is_empty() {
                "no-facts:module"
            } else {
                "no-facts:binding"
            });
        };
        let Some(cid) = self.class_id(id) else {
            return Some(if recv.class_ids.is_empty() {
                "no-facts:class-set-empty"
            } else {
                "no-facts:class-set-many"
            });
        };
        let Some(fc) = facts.classes.get(&cid) else {
            return Some("no-facts:class");
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

    /// Why `let id = <birth>` is not a candidate.
    fn let_position(&self, id: u32, init: &Expr) -> &'static str {
        match init {
            Expr::New { args, .. } if args.iter().any(|a| matches!(a, Expr::LiteralChunks(_))) => {
                "let-chunked"
            }
            _ if self.boxed.contains(&id) => "let-boxed",
            _ if self.globals.contains_key(&id) => "let-module-global",
            _ => "let-other",
        }
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
                    let at = match self.cand_kind(*id) {
                        Kind::New => *byte_offset,
                        _ => self.next_ordinal(),
                    };
                    self.defs.insert(*id, at);
                    for a in args {
                        self.expr(a, Some(init));
                    }
                }
                e if birth(e).is_some() => {
                    let pos = self.let_position(*id, e);
                    if matches!(e, Expr::New { .. }) || pos != "let-other" {
                        self.birth_site(e, pos, String::new());
                    } else {
                        self.literal_let(*id, e);
                    }
                }
                e => match self.cand(e) {
                    Some(c) => self.block(c, "aliased-to-local"),
                    None => self.expr(e, None),
                },
            },
            Stmt::Let { init: None, .. } => {}
            Stmt::Return(Some(e)) => match self.cand(e) {
                Some(c) => self.block(c, "returned"),
                None if birth(e).is_some() => self.birth_site(e, "return", String::new()),
                None => self.expr(e, None),
            },
            Stmt::Throw(e) => match self.cand(e) {
                Some(c) => self.block(c, "thrown"),
                None if birth(e).is_some() => self.birth_site(e, "thrown", String::new()),
                None => self.expr(e, None),
            },
            Stmt::Expr(e) if birth(e).is_some() => {
                self.birth_site(e, "expression-statement", String::new())
            }
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

    fn next_ordinal(&mut self) -> u32 {
        self.ordinal += 1;
        self.ordinal
    }

    /// `let id = <literal or closure>` on a plain local: a candidate.
    fn literal_let(&mut self, id: u32, e: &Expr) {
        let Some((kind, class)) = birth(e) else {
            return;
        };
        let (slots, keys) = match e {
            Expr::Object(props) => (
                Some(props.len()),
                props.iter().map(|(k, _)| k.clone()).collect(),
            ),
            Expr::Array(elements, _) => (Some(elements.len()), Vec::new()),
            Expr::Closure { captures, .. } => (Some(captures.len()), Vec::new()),
            _ => (None, Vec::new()),
        };
        let at = self.next_ordinal();
        self.defs.insert(id, at);
        self.candidates.insert(
            id,
            Cand {
                kind,
                class,
                slots,
                keys,
                new: false,
            },
        );
        self.children(e);
    }

    /// The parts of a birth: its arguments, fields or elements (a nested
    /// birth gets its position from this parent), or a closure's captures.
    fn children(&mut self, e: &Expr) {
        if let Expr::Closure { .. } = e {
            let mut refs = Vec::new();
            perry_hir::analysis::collect_local_refs_expr(e, &mut refs, &mut HashSet::new());
            for id in refs {
                if self.candidates.contains_key(&id) {
                    self.block(id, "captured-by-closure");
                }
            }
            return;
        }
        let mut kids = Vec::new();
        perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
        for k in kids {
            self.expr(k, Some(e));
        }
    }

    /// A birth that is not a candidate, at `position`.
    fn birth_site(&mut self, e: &Expr, position: &str, detail: String) {
        let Some((kind, class)) = birth(e) else {
            return;
        };
        let offset = match e {
            Expr::New { byte_offset, .. } if kind == Kind::New => *byte_offset,
            _ => self.next_ordinal(),
        };
        self.rows.push((
            kind,
            class,
            offset,
            None,
            format!("none:{position}"),
            detail,
        ));
        self.children(e);
    }

    /// A candidate or a birth as argument `i` whose fate is `fate`.
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
        } else if birth(a).is_some() {
            let detail = fate.map_or(callee.to_string(), |f| format!("{callee}:{}", fate_word(f)));
            self.birth_site(a, "argument", detail);
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
            _ if birth(e).is_some() => match parent.map(kind) {
                Some(k) if k.contains("Call") || k.starts_with("Native") => {
                    self.birth_site(e, "argument", format!("runtime/builtin:{k}"))
                }
                _ => self.birth_site(e, &position(parent), String::new()),
            },
            Expr::PropertyGet {
                object, property, ..
            } if self.cand(object).is_some() => {
                let c = self.cand(object).expect("candidate");
                match self.cand_kind(c) {
                    _ if self.candidates[&c].new => {
                        let verdict = self.m.facts.locals.get(&c).map(|r| {
                            crate::expr::wpfacts_access::facts_read(&self.m.facts, r, property)
                        });
                        match verdict {
                            Some(Ok(true)) => {}
                            Some(Ok(false)) => self.block(c, "read-guarded"),
                            _ => self.block(c, "read-unproven"),
                        }
                    }
                    Kind::Object if self.candidates[&c].keys.iter().any(|k| k == property) => {}
                    Kind::Array if property == "length" => {}
                    _ => self.block(c, "read-unproven"),
                }
            }
            Expr::IndexGet { object, index, .. } if self.cand(object).is_some() => {
                let c = self.cand(object).expect("candidate");
                let constant = super::escape_arrays::const_index(index).is_some();
                if !(self.cand_kind(c) == Kind::Array && constant) {
                    self.block(c, "read-computed-key");
                }
                self.expr(index, Some(e));
            }
            Expr::Call { callee, args, .. } => {
                let (fn_id, what) = match callee.as_ref() {
                    Expr::FuncRef(f) => (Some(*f), "direct function"),
                    Expr::LocalGet(c)
                        if self
                            .candidates
                            .get(c)
                            .is_some_and(|k| k.kind == Kind::Closure) =>
                    {
                        (None, "local closure")
                    }
                    Expr::PropertyGet { object, .. } => {
                        match self.cand(object) {
                            Some(c) => self.block(c, "call-kept:method-receiver"),
                            None if birth(object).is_some() => {
                                self.birth_site(object, "method-receiver", String::new())
                            }
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

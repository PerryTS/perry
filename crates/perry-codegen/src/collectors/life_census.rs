//! LIFE D1: the frame-placement census (compiler-internal, off by default).
//!
//! With `PERRY_LIFE_CENSUS=<path>` set at compile time (the driver then
//! compiles uncached, as with `PERRY_NO_CACHE=1`), every body's escape
//! analyses (`hir_facts`: news, array and object literals) also classify
//! each heap birth of the body, and append one TSV row per birth to
//! `<path>`: `module, function, class, offset, slots, verdict, detail, kind,
//! receiving, outer`. The last two (LIFE C1) say what receives a birth that
//! is not bound to a local and the verdict of the birth holding it; their
//! vocabulary is in `life_census_sides`. Rows are written when the module's
//! codegen ends: a returned birth's side reads every caller body.
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
//!   is not one class; `class`, no layout facts for that class), `escaped` (the module has
//!   facts but no receiver or class facts at all: its values escaped),
//!   `unreached-body` (the solve gave other bindings of the module a
//!   representation, not this one: it never saw the body), `too-many-slots` (more than
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

use std::collections::{BTreeMap, HashMap, HashSet};
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
/// guard; a guarded one is a bet on its site; a store `this.k = v` is
/// code-free when `wpfacts_access::facts_store` admits it; call bodies are
/// direct function references only (slice 1).
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
    /// `this.property = v` in `func`: `facts_store` on `func`'s `this`
    /// fact, the predicate `prove` gates the store on. Never a bet.
    fn store(&self, func: FuncId, property: &str) -> Option<Proof> {
        let recv = self.facts.this_of.get(&func)?;
        crate::expr::wpfacts_access::facts_store(self.facts, recv, property)
            .ok()
            .map(|()| Proof::Proven)
    }
    fn targets(&self, callee: &Expr) -> Option<(Vec<FuncId>, Proof)> {
        direct_target(callee)
    }
}

struct ModuleCensus {
    name: String,
    facts: Arc<ElisionFacts>,
    fates: Fates,
    /// Body address -> the body.
    bodies: HashMap<usize, Body>,
    classes: HashMap<String, bool>,
    /// Class id -> its static methods (`life_census_sides::resolve`).
    statics: sides::Statics,
    /// (class, static method) -> its body, for HIR's `StaticMethodCall`.
    static_by_name: HashMap<(String, String), FuncId>,
    callers: sides::Callers,
    /// Function -> call site -> what the caller does with the result.
    uses: Mutex<HashMap<FuncId, BTreeMap<(u32, u32), String>>>,
    /// Rows of the module, written when its codegen ends.
    pending: Mutex<Vec<(String, Row)>>,
    seen: Mutex<HashSet<(String, String, String)>>,
}

/// One body the census can name.
#[derive(Clone)]
pub(crate) struct Body {
    label: String,
    /// Async or generator.
    suspends: bool,
    func: FuncId,
    /// A module function (its callers are visible), not a method,
    /// constructor or closure.
    direct: bool,
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
        self.0.flush();
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
        let callers = sides::Callers::of(hir);
        let mut functions: Vec<&perry_hir::Function> = hir.functions.iter().collect();
        functions.extend(callers.closures.iter());
        let classes_ref: Vec<&perry_hir::Class> = hir.classes.iter().collect();
        let fates = Fates::compute(&functions, &classes_ref, &CodegenProof { facts });
        write_params(&hir.name, &functions, &classes_ref, &fates, &callers);
        let mut bodies = HashMap::new();
        let mut note = |f: &perry_hir::Function, label: String, direct: bool| {
            bodies.insert(
                f.body.as_ptr() as usize,
                Body {
                    label,
                    suspends: f.is_async || f.is_generator,
                    func: f.id,
                    direct,
                },
            );
        };
        for f in &hir.functions {
            note(f, f.name.clone(), true);
        }
        let mut classes = HashMap::new();
        let mut statics: sides::Statics = HashMap::new();
        let mut static_by_name = HashMap::new();
        for c in &hir.classes {
            if let Some(k) = &c.constructor {
                note(k, format!("{}.constructor", c.name), false);
            }
            for m in c.methods.iter().chain(&c.static_methods) {
                note(m, format!("{}.{}", c.name, m.name), false);
            }
            for m in &c.static_methods {
                statics.entry(c.id).or_default().insert(m.name.clone(), m.id);
                static_by_name.insert((c.name.clone(), m.name.clone()), m.id);
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
        for &(at, func, suspends) in &callers.closure_bodies {
            bodies.insert(
                at,
                Body {
                    label: format!("closure#{func}"),
                    suspends,
                    func,
                    direct: false,
                },
            );
        }
        ModuleCensus {
            name: hir.name.clone(),
            facts: facts.clone(),
            fates,
            bodies,
            classes,
            statics,
            static_by_name,
            callers,
            uses: Mutex::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Column 9 of the returned births, once every caller body is walked.
    fn finish(&self, rows: Vec<Row>) -> Vec<Row> {
        let uses = self.uses.lock().expect("census uses");
        rows.into_iter()
            .map(|mut r| {
                if let Some(f) = r.6.strip_prefix("return:@").and_then(|f| f.parse().ok()) {
                    r.6 = sides::return_side(&self.callers, &uses, f);
                }
                r
            })
            .collect()
    }

    /// Writes the module's rows (`Registered`'s drop: codegen has ended).
    fn flush(&self) {
        let Some(out) = path() else { return };
        let pending = std::mem::take(&mut *self.pending.lock().expect("census rows"));
        let (labels, rows): (Vec<String>, Vec<Row>) = pending.into_iter().unzip();
        let mut text = String::new();
        for (label, (kind, class, offset, slots, verdict, detail, side, outer)) in
            labels.into_iter().zip(self.finish(rows))
        {
            let offset = match kind {
                Kind::New => offset.to_string(),
                _ => format!("#{offset}"),
            };
            let slots = slots.map_or("-".to_string(), |s| s.to_string());
            text.push_str(&format!(
                "{}\t{label}\t{class}\t{offset}\t{slots}\t{verdict}\t{detail}\t{}\t{side}\t{outer}\n",
                self.name,
                kind.word()
            ));
        }
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
    if path().is_none() {
        return;
    }
    let key = stmts.as_ptr() as usize;
    let found = registry()
        .lock()
        .expect("census registry")
        .iter()
        .find_map(|m| m.bodies.get(&key).map(|b| (m.clone(), b.clone())));
    let (module, body) = match found {
        Some((m, b)) => (m, Some(b)),
        None => match CURRENT.with(|c| c.borrow().clone()) {
            Some(m) => (m, None),
            None => return,
        },
    };
    let label = body.as_ref().map_or("<init>".to_string(), |b| b.label.clone());
    let rows = classify(&module, stmts, boxed_vars, module_globals, scalar, body.as_ref());
    let mut seen = module.seen.lock().expect("census seen");
    let mut pending = module.pending.lock().expect("census rows");
    for row in rows {
        let offset = match row.0 {
            Kind::New => row.2.to_string(),
            _ => format!("#{}", row.2),
        };
        if seen.insert((label.clone(), offset, row.1.clone())) {
            pending.push((label.clone(), row));
        }
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
/// body walk), slots, verdict, detail, receiving side, outer verdict.
type Row = (Kind, String, u32, Option<usize>, String, String, String, String);

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
    /// `let x = f(...)` of a module function `f`: not a birth here; its
    /// uses say what that caller does with `f`'s result (the return side).
    result_of: Option<(FuncId, (u32, u32))>,
}

/// The census rows of one body: every candidate's verdict, and every birth
/// that is not a candidate.
fn classify(
    module: &ModuleCensus,
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    scalar: &Scalar<'_>,
    body: Option<&Body>,
) -> Vec<Row> {
    let suspends = body.is_some_and(|b| b.suspends);
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
                result_of: None,
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
        body,
        outer: None,
        pending_use: None,
    };
    for s in stmts {
        c.stmt(s);
    }
    let mut rows = std::mem::take(&mut c.rows);
    let mut cand_verdicts = HashMap::new();
    for (id, cand) in &c.candidates {
        if let Some((f, site)) = cand.result_of {
            let used = c
                .first
                .get(id)
                .map_or("drop".to_string(), |r| format!("kept:local:heap:{r}"));
            module.record_use(f, site, used);
            continue;
        }
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
        cand_verdicts.insert(format!("@cand:{id}"), verdict.clone());
        rows.push((
            cand.kind,
            cand.class.clone(),
            offset,
            slots,
            verdict,
            String::new(),
            "-".to_string(),
            "-".to_string(),
        ));
    }
    let row_verdicts: Vec<String> = rows.iter().map(|r| r.4.clone()).collect();
    for r in &mut rows {
        if let Some(i) = r.7.strip_prefix("@row:").and_then(|i| i.parse::<usize>().ok()) {
            r.7 = row_verdicts[i].clone();
        } else if let Some(v) = cand_verdicts.get(&r.7) {
            r.7 = v.clone();
        }
    }
    rows
}

impl ModuleCensus {
    /// A caller's use of a module function's result at one call site.
    fn record_use(&self, f: FuncId, site: (u32, u32), used: String) {
        self.uses
            .lock()
            .expect("census uses")
            .entry(f)
            .or_default()
            .entry(site)
            .or_insert(used);
    }
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
    body: Option<&'a Body>,
    /// While a birth's parts are walked: its row (`@row:<index>`) or
    /// candidate (`@cand:<id>`), the outer column of a part that is a birth.
    outer: Option<String>,
    /// The statement's use of the result of the call it consists of.
    pending_use: Option<&'static str>,
}

fn kind(e: &Expr) -> String {
    let text = format!("{e:?}");
    text.split(|c: char| c == '(' || c == ' ' || c == '{')
        .next()
        .unwrap_or("?")
        .to_string()
}

/// `<census>.params.tsv`: one row per parameter (and `this` of a method or
/// constructor) of every body with a fate: module, body, function id,
/// `<index>:<name>` or `this`, called directly here (`direct`) / referenced
/// other than as a callee (`escapes`), and the fate with its reason.
fn write_params(
    module: &str,
    functions: &[&perry_hir::Function],
    classes: &[&perry_hir::Class],
    fates: &Fates,
    callers: &sides::Callers,
) {
    let Some(out) = path() else { return };
    let text = params_text(module, functions, classes, fates, callers);
    if text.is_empty() {
        return;
    }
    static WRITE: Mutex<()> = Mutex::new(());
    let _w = WRITE.lock().expect("census params write");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{out}.params.tsv"))
    {
        let _ = f.write_all(text.as_bytes());
    }
}

/// The rows of `write_params`.
fn params_text(
    module: &str,
    functions: &[&perry_hir::Function],
    classes: &[&perry_hir::Class],
    fates: &Fates,
    callers: &sides::Callers,
) -> String {
    let mut bodies: Vec<(String, &perry_hir::Function, bool)> =
        functions.iter().map(|f| (f.name.clone(), *f, false)).collect();
    for c in classes {
        if let Some(k) = &c.constructor {
            bodies.push((format!("{}.constructor", c.name), k, true));
        }
        for m in &c.methods {
            bodies.push((format!("{}.{}", c.name, m.name), m, true));
        }
        for m in &c.static_methods {
            bodies.push((format!("{}.static.{}", c.name, m.name), m, false));
        }
    }
    let mut text = String::new();
    for (label, f, this) in bodies {
        let Some(ff) = fates.of_fn(f.id) else { continue };
        let use_ = match (callers.sites.contains_key(&f.id), callers.escapes.contains(&f.id)) {
            (true, true) => "direct+escapes",
            (true, false) => "direct",
            (false, true) => "escapes",
            (false, false) => "-",
        };
        let mut row = |param: String, p: ParamFate| {
            text.push_str(&format!(
                "{module}\t{label}\t{}\t{param}\t{use_}\t{}\n",
                f.id,
                fate_why(p)
            ));
        };
        for (i, (p, pf)) in f.params.iter().zip(&ff.params).enumerate() {
            row(format!("{i}:{}", p.name), *pf);
        }
        if this {
            row("this".to_string(), ff.this);
        }
    }
    text
}

/// Column 9's fate: `fate_word`, and for `Kept` its first keeping use in
/// the body (`Kept:<why>`).
fn fate_why(f: ParamFate) -> String {
    match (f.fate, f.why) {
        (Fate::Kept, Some(w)) => format!("Kept:{w}"),
        _ => fate_word(f).to_string(),
    }
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
            return Some(if facts.is_empty() {
                "no-facts:module"
            } else if facts.locals.is_empty() && facts.classes.is_empty() {
                "escaped"
            } else if !facts.local_reps.is_empty() && !facts.local_reps.contains_key(&id) {
                "unreached-body"
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
                    let saved = self.outer.replace(format!("@cand:{id}"));
                    for a in args {
                        self.expr(a, Some(init));
                    }
                    self.outer = saved;
                }
                e if birth(e).is_some() => {
                    let pos = self.let_position(*id, e);
                    if matches!(e, Expr::New { .. }) || pos != "let-other" {
                        self.birth_site(e, pos, String::new(), "-".to_string(), None);
                    } else {
                        self.literal_let(*id, e);
                    }
                }
                Expr::Call {
                    callee,
                    byte_offset,
                    byte_end,
                    ..
                } if matches!(callee.as_ref(), Expr::FuncRef(_))
                    && !self.boxed.contains(id)
                    && !self.globals.contains_key(id)
                    && !self.candidates.contains_key(id) =>
                {
                    let Expr::FuncRef(f) = callee.as_ref() else {
                        unreachable!("matched above")
                    };
                    self.candidates.insert(
                        *id,
                        Cand {
                            kind: Kind::New,
                            class: String::new(),
                            slots: None,
                            keys: Vec::new(),
                            new: true,
                            result_of: Some((*f, (*byte_offset, *byte_end))),
                        },
                    );
                    self.statement_call(init, "result");
                }
                e => match self.cand(e) {
                    Some(c) => self.block(c, "aliased-to-local"),
                    None => self.expr(e, None),
                },
            },
            Stmt::Let { init: None, .. } => {}
            Stmt::Return(Some(e)) => match self.cand(e) {
                Some(c) => self.block(c, "returned"),
                None if birth(e).is_some() => {
                    let side = match self.body {
                        Some(b) if b.direct => format!("return:@{}", b.func),
                        Some(_) => "return:not-a-direct-function".to_string(),
                        None => "return:unidentified-body".to_string(),
                    };
                    self.birth_site(e, "return", String::new(), side, None)
                }
                None => self.statement_call(e, "kept:returned"),
            },
            Stmt::Throw(e) => match self.cand(e) {
                Some(c) => self.block(c, "thrown"),
                None if birth(e).is_some() => {
                    self.birth_site(e, "thrown", String::new(), "-".to_string(), None)
                }
                None => self.expr(e, None),
            },
            Stmt::Expr(e) if birth(e).is_some() => {
                self.birth_site(e, "expression-statement", String::new(), "-".to_string(), None)
            }
            Stmt::Expr(e) => self.statement_call(e, "drop"),
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

    /// A statement's own expression, which uses the result of the call it
    /// is (if it is one) as `used`.
    fn statement_call(&mut self, e: &Expr, used: &'static str) {
        self.pending_use = Some(used);
        self.expr(e, None);
        self.pending_use = None;
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
                result_of: None,
            },
        );
        let saved = self.outer.replace(format!("@cand:{id}"));
        self.children(e);
        self.outer = saved;
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

    /// A birth that is not a candidate, at `position`, received by `side`;
    /// `parent`: the expression holding it, when that is a birth.
    fn birth_site(
        &mut self,
        e: &Expr,
        position: &str,
        detail: String,
        side: String,
        parent: Option<&Expr>,
    ) {
        let Some((kind, class)) = birth(e) else {
            return;
        };
        let offset = match e {
            Expr::New { byte_offset, .. } if kind == Kind::New => *byte_offset,
            _ => self.next_ordinal(),
        };
        let outer = match parent {
            Some(p) if birth(p).is_some() => self.outer.clone().unwrap_or_else(|| "-".into()),
            _ => "-".to_string(),
        };
        self.rows.push((
            kind,
            class,
            offset,
            None,
            format!("none:{position}"),
            detail,
            side,
            outer,
        ));
        let saved = self.outer.replace(format!("@row:{}", self.rows.len() - 1));
        self.children(e);
        self.outer = saved;
    }

    /// A candidate or a birth as argument `i` whose fate is `fate`.
    fn argument(&mut self, a: &Expr, fate: Option<ParamFate>, callee: &str, side: String) {
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
            self.birth_site(a, "argument", detail, side, None);
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
                    let side = sides::node_side(self.m, parent.expect("parent"), e);
                    self.birth_site(e, "argument", format!("runtime/builtin:{k}"), side, parent)
                }
                _ => {
                    let side = match parent {
                        Some(Expr::New {
                            class_name, args, ..
                        }) => {
                            let i = args.iter().position(|a| std::ptr::eq(a, e)).unwrap_or(0);
                            sides::construct_side(self.m, class_name, i)
                        }
                        Some(Expr::Object(_) | Expr::ObjectSpread { .. }) => {
                            "literal-field".to_string()
                        }
                        Some(
                            Expr::Array(..)
                            | Expr::ArraySpread(_)
                            | Expr::PropertySet { .. }
                            | Expr::IndexSet { .. }
                            | Expr::PropertyGet { .. }
                            | Expr::IndexGet { .. },
                        )
                        | None => "-".to_string(),
                        Some(p) => sides::node_side(self.m, p, e),
                    };
                    self.birth_site(e, &position(parent), String::new(), side, parent)
                }
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
                            None if birth(object).is_some() => self.birth_site(
                                object,
                                "method-receiver",
                                String::new(),
                                "-".to_string(),
                                None,
                            ),
                            None => self.expr(object, Some(callee)),
                        }
                        (None, "method")
                    }
                    other => {
                        self.expr(other, Some(e));
                        (None, "unknown target")
                    }
                };
                if let Expr::FuncRef(f) = callee.as_ref() {
                    let used = match self.pending_use.take().filter(|_| parent.is_none()) {
                        Some("result") => None,
                        Some(u) => Some(u.to_string()),
                        None => Some(match parent {
                            None => "kept:statement-other".to_string(),
                            Some(p) if kind(p).contains("Call") => "kept:argument".to_string(),
                            Some(_) => format!("kept:{}", position(parent)),
                        }),
                    };
                    if let (Some(u), Expr::Call {
                        byte_offset,
                        byte_end,
                        ..
                    }) = (used, e)
                    {
                        self.m.record_use(*f, (*byte_offset, *byte_end), u);
                    }
                }
                self.pending_use = None;
                let this_fn = self.body.map(|b| b.func);
                for (i, a) in args.iter().enumerate() {
                    let fate = fn_id.map(|f| self.m.fates.call_arg(&[f], i));
                    let side = sides::call_side(self.m, this_fn, callee, i);
                    self.argument(a, fate, what, side);
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

#[path = "life_census_sides.rs"]
mod sides;

#[cfg(test)]
#[path = "life_census_tests.rs"]
mod tests;

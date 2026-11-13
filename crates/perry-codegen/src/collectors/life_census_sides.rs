//! LIFE C1: what RECEIVES a birth that is not bound to a local (census
//! column 9, `receiving`), and the verdict of the birth that holds it
//! (column 10, `outer`). Compiler-internal, written only with
//! `PERRY_LIFE_CENSUS`; nothing here changes what codegen emits.
//!
//! Column 9, a fixed vocabulary:
//! * `compiled:<how>:<fate>`: an argument of a call whose bodies are known,
//!   resolved as the lowering's proven calls resolve them (`proven_call`,
//!   `wpfacts_access`): `direct` (a function reference), `local-bodies` (a
//!   local the facts prove holds only these bodies), `field-bodies` (a
//!   field of the receiver's proven classes holding only bodies, read on
//!   `facts_read`'s unguarded answer), `static-method` (a local holding one
//!   class object whose own static the facts prove), `static` (HIR's
//!   `StaticMethodCall` of a class of this module). `<fate>` is that
//!   parameter's fate over every body (`perry_transform::effects`, with
//!   closures that capture nothing): `Dropped`, `Dropped(bet)`, `Returned`,
//!   `KeptByReceiver`, `Kept`, or `Kept(no-body)` when some body has no
//!   fate here (another module's, or a closure that captures);
//! * `unknown-target:<reason>`: the call's bodies are not known:
//!   `module-no-facts`, `other-module` (an import), `local-not-only-bodies`,
//!   `receiver-expression` (neither a local nor `this`), `receiver-no-facts`,
//!   `receiver-top` (empty class set), `several-classes` (more than four),
//!   `receiver-guarded` (the binding may hold other values, or interpreted
//!   code runs), `receiver-nullable`, `private-key`, `key-not-a-field`,
//!   `field-declared-or-reserved`, `field-not-only-bodies`,
//!   `static-unproven`, `static-not-declared`, `computed-callee`,
//!   `callee-expression`, `super`, `spread-call`, `new-dynamic`;
//! * `builtin:<name>`: runtime code receives it: `global.<key>` for a
//!   method of a global object (`console.log` is `global.log`: HIR keeps
//!   only the key), `<module>.<method>` for a native method call, else the
//!   HIR node (`ObjectKeys`, `ArrayFrom`, ...): a birth that is an operand
//!   of such a node (position `other:<node>`) gets this side as well;
//! * `literal-field`: a field value of an object literal (HIR's `new
//!   __AnonShape_*(values)`, or a remaining `{...}`);
//! * `new:<fate>`: an argument of `new C(...)`, the constructor's fate
//!   (`new:Kept(no-body)` for a class of another module);
//!   `new:kept-by-new-object` when the constructor stores it into a field
//!   of the new object (fate `KeptByReceiver`);
//! * `return:<what the visible callers do>` for a returned birth:
//!   `exported`, `function-escapes` (the function is used as a value),
//!   `no-visible-caller`, `all-drop` (every call's result is dropped or
//!   bound to a local whose uses block nothing), `kept:<use>` (the first
//!   keeping use: `local:<verdict of that local>`, `returned`, or the
//!   result's position), `kept:unobserved-caller` (a caller body the census
//!   did not see), `not-a-direct-function` (a method, constructor or
//!   closure), `unidentified-body`;
//! * `-`: none of these (a store, an operator, a statement position).
//!
//! Column 10: for a birth that is a field, element or constructor argument
//! of another birth, that birth's verdict (column 6); else `-`.

use std::collections::{BTreeMap, HashMap, HashSet};

use perry_hir::types::FuncId;
use perry_hir::{Expr, Stmt};
use perry_transform::effects::Fate;

use super::{fate_why, kind, ModuleCensus};
use crate::codegen::ElisionFacts;
use crate::expr::wpfacts_access::{facts_read, facts_static};
use crate::lower_call::proven_call::{field_call_bodies, value_call_bodies};

/// Class id -> its own static methods by name.
pub(super) type Statics = HashMap<u32, HashMap<String, FuncId>>;

/// The bodies a call of `callee` runs, as the lowering's proven calls
/// resolve them, with how; else the fixed reason it does not resolve.
/// `this_fn`: the function whose `this` a `this.k(...)` reads.
pub(super) fn resolve(
    facts: &ElisionFacts,
    statics: &Statics,
    this_fn: Option<FuncId>,
    callee: &Expr,
) -> Result<(&'static str, Vec<FuncId>), &'static str> {
    match callee {
        Expr::FuncRef(f) => Ok(("direct", vec![*f])),
        Expr::ExternFuncRef { .. } | Expr::NativeModuleRef(_) => Err("other-module"),
        _ if facts.is_empty() => Err("module-no-facts"),
        Expr::LocalGet(id) => value_call_bodies(facts, *id)
            .map(|b| ("local-bodies", b.iter().map(|b| b.func_id).collect()))
            .ok_or("local-not-only-bodies"),
        Expr::PropertyGet {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(&cid) = facts.local_classes.get(id) {
                    if !facts_static(facts, cid, property) {
                        return Err("static-unproven");
                    }
                    return statics
                        .get(&cid)
                        .and_then(|m| m.get(property.as_str()))
                        .map(|f| ("static-method", vec![*f]))
                        .ok_or("static-not-declared");
                }
            }
            let recv = match object.as_ref() {
                Expr::LocalGet(id) => facts.locals.get(id),
                Expr::This => this_fn.and_then(|f| facts.this_of.get(&f)),
                Expr::ExternFuncRef { .. } | Expr::NativeModuleRef(_) => {
                    return Err("other-module")
                }
                _ => return Err("receiver-expression"),
            }
            .ok_or("receiver-no-facts")?;
            match facts_read(facts, recv, property) {
                Err(_) if property.starts_with('#') => return Err("private-key"),
                Err(_) if recv.class_ids.is_empty() => return Err("receiver-top"),
                Err(_) if recv.class_ids.len() > 4 => return Err("several-classes"),
                Err(_) => return Err("key-not-a-field"),
                Ok(false) if recv.checked || facts.interp_guard => return Err("receiver-guarded"),
                Ok(false) => return Err("field-declared-or-reserved"),
                Ok(true) => {}
            }
            if recv.nullable {
                return Err("receiver-nullable");
            }
            field_call_bodies(facts, &recv.class_ids, property)
                .map(|b| ("field-bodies", b.iter().map(|b| b.func_id).collect()))
                .ok_or("field-not-only-bodies")
        }
        Expr::IndexGet { .. } => Err("computed-callee"),
        _ => Err("callee-expression"),
    }
}

/// The fate of argument `i` over `bodies`.
fn fate_over(m: &ModuleCensus, bodies: &[FuncId], i: usize) -> String {
    if bodies.iter().any(|b| m.fates.of_fn(*b).is_none()) {
        return "Kept(no-body)".to_string();
    }
    fate_why(m.fates.call_arg(bodies, i))
}

/// Column 9 of argument `i` of `callee(...)`.
pub(super) fn call_side(
    m: &ModuleCensus,
    this_fn: Option<FuncId>,
    callee: &Expr,
    i: usize,
) -> String {
    if let Expr::PropertyGet {
        object, property, ..
    } = callee
    {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) {
            return format!("builtin:global.{property}");
        }
    }
    match resolve(&m.facts, &m.statics, this_fn, callee) {
        Ok((how, bodies)) => format!("compiled:{how}:{}", fate_over(m, &bodies, i)),
        Err(why) => format!("unknown-target:{why}"),
    }
}

/// Column 9 of argument `i` of `new class(...)`.
pub(super) fn construct_side(m: &ModuleCensus, class: &str, i: usize) -> String {
    if class.starts_with("__AnonShape_") {
        return "literal-field".to_string();
    }
    if !m.classes.contains_key(class) {
        return "new:Kept(no-body)".to_string();
    }
    let f = m.fates.construction_arg(class, i);
    if f.fate == Fate::KeptByReceiver {
        "new:kept-by-new-object".to_string()
    } else {
        format!("new:{}", fate_why(f))
    }
}

/// HIR nodes that are language operations, stores or literal parts, not
/// runtime library code: a birth under one has no receiving side here.
const LANGUAGE: &[&str] = &[
    "Binary",
    "Unary",
    "Compare",
    "Logical",
    "Conditional",
    "TypeOf",
    "Void",
    "InstanceOf",
    "In",
    "Await",
    "Yield",
    "Sequence",
    "Delete",
    "LocalSet",
    "GlobalSet",
    "ScopedTemp",
    "WithSet",
    "PutValueSet",
    "PropertyUpdate",
    "IndexUpdate",
    "StaticFieldSet",
    "SuperPropertySet",
    "ObjectSuperPropertySet",
    "ClassEnvSet",
    "LiteralSlotInit",
    "LiteralChunks",
    "LiteralChunkObject",
    "IterResultSet",
    "TemplateStringCoerce",
    "StringCoerce",
    "NumberCoerce",
    "BooleanCoerce",
    "ObjectCoerce",
];

/// Column 9 of `child`, an operand of the call-like or dedicated node
/// `parent` (not a `Call`, not a `new`, not a literal).
pub(super) fn node_side(m: &ModuleCensus, parent: &Expr, child: &Expr) -> String {
    match parent {
        Expr::NativeMethodCall { module, method, .. } => format!("builtin:{module}.{method}"),
        Expr::StaticMethodCall {
            class_name,
            method_name,
            args,
        } => {
            let i = args
                .iter()
                .position(|a| std::ptr::eq(a, child))
                .unwrap_or(0);
            match m
                .static_by_name
                .get(&(class_name.clone(), method_name.clone()))
            {
                Some(f) => format!("compiled:static:{}", fate_over(m, &[*f], i)),
                None => "unknown-target:other-module".to_string(),
            }
        }
        Expr::SuperCall(..)
        | Expr::SuperCallSpread { .. }
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. } => "unknown-target:super".to_string(),
        Expr::CallSpread { .. } => "unknown-target:spread-call".to_string(),
        Expr::NewDynamic { .. } | Expr::NewDynamicSpread { .. } => {
            "unknown-target:new-dynamic".to_string()
        }
        p => {
            let k = kind(p);
            if LANGUAGE.contains(&k.as_str()) {
                "-".to_string()
            } else {
                format!("builtin:{k}")
            }
        }
    }
}

/// The module-wide call facts the return side reads, from the HIR.
#[derive(Default)]
pub(super) struct Callers {
    /// Function -> its direct call sites (byte offset, end).
    pub sites: HashMap<FuncId, HashSet<(u32, u32)>>,
    /// Functions referenced other than as a call's callee.
    pub escapes: HashSet<FuncId>,
    pub exported: HashSet<FuncId>,
    /// Closures that capture nothing, as functions (for the fates).
    pub closures: Vec<perry_hir::Function>,
    /// Every closure body: (address, function id, async or generator).
    pub closure_bodies: Vec<(usize, FuncId, bool)>,
}

impl Callers {
    pub(super) fn of(hir: &perry_hir::Module) -> Self {
        let mut c = Callers::default();
        let mut callee_refs: HashMap<FuncId, usize> = HashMap::new();
        let mut refs: HashMap<FuncId, usize> = HashMap::new();
        let mut visit = |e: &Expr| match e {
            Expr::Call {
                callee,
                byte_offset,
                byte_end,
                ..
            } => {
                if let Expr::FuncRef(f) = callee.as_ref() {
                    *callee_refs.entry(*f).or_default() += 1;
                    c.sites
                        .entry(*f)
                        .or_default()
                        .insert((*byte_offset, *byte_end));
                }
            }
            Expr::FuncRef(f) => *refs.entry(*f).or_default() += 1,
            Expr::Closure {
                func_id,
                body,
                is_async,
                is_generator,
                captures,
                captures_this,
                captures_new_target,
                ..
            } if !(captures.is_empty() && !captures_this && !captures_new_target) => c
                .closure_bodies
                .push((body.as_ptr() as usize, *func_id, *is_async || *is_generator)),
            Expr::Closure {
                func_id,
                params,
                return_type,
                body,
                captures,
                captures_this,
                captures_new_target,
                is_async,
                is_generator,
                ..
            } if captures.is_empty() && !captures_this && !captures_new_target => {
                c.closure_bodies.push((
                    body.as_ptr() as usize,
                    *func_id,
                    *is_async || *is_generator,
                ));
                c.closures.push(perry_hir::Function {
                    id: *func_id,
                    name: format!("closure#{func_id}"),
                    type_params: Vec::new(),
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body: body.clone(),
                    is_async: *is_async,
                    is_generator: *is_generator,
                    is_strict: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                    was_plain_async: false,
                    was_unrolled: false,
                })
            }
            _ => {}
        };
        for b in bodies(hir) {
            visit_body(b, &mut visit);
        }
        for (f, n) in refs {
            if n > callee_refs.get(&f).copied().unwrap_or(0) {
                c.escapes.insert(f);
            }
        }
        c.exported = hir
            .functions
            .iter()
            .filter(|f| f.is_exported)
            .map(|f| f.id)
            .collect();
        c
    }
}

/// Every statement list of the module that codegen compiles as a body.
fn bodies(hir: &perry_hir::Module) -> Vec<&[Stmt]> {
    let mut out: Vec<&[Stmt]> = vec![&hir.init];
    out.extend(hir.functions.iter().map(|f| f.body.as_slice()));
    for c in &hir.classes {
        out.extend(c.constructor.iter().map(|f| f.body.as_slice()));
        out.extend(
            c.methods
                .iter()
                .chain(&c.static_methods)
                .map(|f| f.body.as_slice()),
        );
        out.extend(
            c.getters
                .iter()
                .chain(&c.setters)
                .map(|(_, f)| f.body.as_slice()),
        );
    }
    out
}

fn visit_body<'a>(stmts: &'a [Stmt], f: &mut dyn FnMut(&'a Expr)) {
    for s in stmts {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            visit_expr(e, f);
            false
        });
    }
}

fn visit_expr<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    f(e);
    if let Expr::Closure { body, .. } = e {
        visit_body(body, f);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, f));
}

/// Column 9 of a birth `return`ed by function `func`, from what its visible
/// callers do with the result (`uses`: call site -> `drop` or `kept:<use>`,
/// as the census walk of the caller bodies recorded them).
pub(super) fn return_side(
    callers: &Callers,
    uses: &HashMap<FuncId, BTreeMap<(u32, u32), String>>,
    func: FuncId,
) -> String {
    if callers.exported.contains(&func) {
        return "return:exported".to_string();
    }
    if callers.escapes.contains(&func) {
        return "return:function-escapes".to_string();
    }
    let Some(sites) = callers.sites.get(&func).filter(|s| !s.is_empty()) else {
        return "return:no-visible-caller".to_string();
    };
    let empty = BTreeMap::new();
    let seen = uses.get(&func).unwrap_or(&empty);
    if let Some(k) = seen.values().find(|u| *u != "drop") {
        return format!("return:{k}");
    }
    if sites.iter().any(|s| !seen.contains_key(s)) {
        return "return:kept:unobserved-caller".to_string();
    }
    "return:all-drop".to_string()
}

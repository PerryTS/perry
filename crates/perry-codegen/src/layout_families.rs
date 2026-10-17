//! Object model case 2c (OBJECT_MODEL_2026-10-13.md §3.2, §3.7): the
//! program's static class families and the global key colouring.
//!
//! **Colouring.** A key read at sites whose receiver nothing proves gets one
//! program-wide slot (its colour) and a bit in every layout record's colour
//! mask. A read site of a coloured key (`expr/layout_read.rs`) tests the
//! receiver layout's bit and loads the colour slot: no call, no site state.
//! The colour of a key is the position it has in most of the program's
//! layouts that hold it. A layout sets a key's bit only when the key really
//! sits at its colour slot there, as data: the runtime decides that from the
//! layout's own immutable key list (`perry-runtime` `object/layout_probe.rs`),
//! so a wrong guess here costs a miss, never a wrong answer. The colours are
//! static data in every module's atom records (`layout_desc::atom`).
//!
//! A **static class family** is a class whose instances the runtime can give
//! their whole layout at birth: every constructor on its chain is a prologue
//! of `this.k = <value>` stores (a derived class adds only a leading
//! `super(...)`). The keys stored before any value that may run code (an
//! inert `<param | literal>`; `FamilyLayout::init_count`) are the instance's
//! initialization, which nothing can observe; every later store is an add the
//! store probe checks (order, prototype chain), like any other. The family's
//! layout is the chain's keys in store order, then its late keys (`late`). Every key is a slot that holds
//! the hole, and so is absent, until a store adds it (§3.2: presence is the
//! hole marker in the slot); its objects keep that layout word for life, and
//! a store that would add a key out of the key order, or a key the layout
//! does not name, goes through the object's overflow (`perry-runtime`
//! `object/layout_desc.rs`).
//!
//! **Slots.** [`assign_slots`] is the one place a family layout's keys get
//! their slot numbers: a subclass takes its parent's slots for every
//! inherited key, a coloured key takes its colour slot where that slot is
//! free, and every other key the lowest free slot. The slots are a
//! permutation of the key positions (the runtime's `register_one`), so the
//! key sequence stays the enumeration order. Every consumer reads the slots
//! from [`FamilyLayout::keys`].
//!
//! Soundness never rests on this analysis alone: the runtime adopts a layout
//! for an instance only when the instance's class id, its runtime parent and
//! its prototype identity are the ones the layout was built for.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use perry_hir::{Class, ClassFieldOrigin, Expr, Module as HirModule, Stmt};

/// One static class family's layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FamilyLayout {
    pub class_id: u32,
    /// The runtime parent class id this layout assumes (0: no heritage).
    pub parent_class_id: u32,
    /// The keys in enumeration (store) order with their slots
    /// ([`assign_slots`]).
    pub keys: Vec<(String, u32)>,
    pub slot_count: u32,
    /// The constructor keys are `keys[..own_count]`; the rest are late keys
    /// (`late`). Every slot holds the hole until its key is written.
    pub own_count: u32,
    /// The constructor keys `keys[..init_count]` are stored before any code
    /// that could observe the instance or change its prototype chain runs
    /// after its birth (the inert prologue of every constructor on the chain,
    /// in evaluation order). Only these take the unchecked initializing store
    /// (`expr/layout_read.rs`); a later constructor key is an add through the
    /// store probe, which checks the order and the chain.
    pub init_count: u32,
    /// The creation split (§3.2: family = creation point): a birth whose
    /// constructor argument `discriminant` is one of the variants' constants
    /// is born on that variant's layout, the keys its creation sites store
    /// (`late::add_variants`); any other birth on `keys`. Every variant
    /// holds the constructor keys at the same slots as `keys`.
    pub discriminant: u32,
    pub variants: Vec<FamilyVariant>,
}

/// One layout of a family's creation split.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FamilyVariant {
    /// The constant (an f64's bits) the birth's argument equals.
    pub key_bits: u64,
    pub keys: Vec<(String, u32)>,
    pub slot_count: u32,
}

/// The slots of a variant's keys: the constructor keys (`keys[..own]`, the
/// same as the default layout's) at the default's slots; a late key at its
/// slot in the default layout or its colour slot where free and inside the
/// variant; every other key the lowest free slot.
fn variant_slots(
    keys: &[String],
    own: usize,
    default: &[(String, u32)],
    colored: &BTreeMap<String, (u32, u32)>,
) -> Vec<u32> {
    let n = keys.len();
    let mut taken = vec![false; n];
    let mut slots: Vec<Option<u32>> = vec![None; n];
    let at = |k: &str| default.iter().find(|(d, _)| d == k).map(|(_, s)| *s);
    for (pos, k) in keys.iter().enumerate() {
        let s = if pos < own { at(k) } else { None };
        if let Some(s) = s.filter(|s| (*s as usize) < n && !taken[*s as usize]) {
            slots[pos] = Some(s);
            taken[s as usize] = true;
        }
    }
    for pre in [true, false] {
        for (pos, k) in keys.iter().enumerate().skip(own) {
            if slots[pos].is_some() {
                continue;
            }
            let want = if pre {
                at(k)
            } else {
                colored.get(k).map(|(_, c)| *c)
            };
            if let Some(s) = want.filter(|s| (*s as usize) >= own && (*s as usize) < n) {
                if !taken[s as usize] {
                    slots[pos] = Some(s);
                    taken[s as usize] = true;
                }
            }
        }
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(pos, s)| {
            s.unwrap_or_else(|| {
                let lo = if pos < own { 0 } else { own };
                let s = (lo..n).find(|s| !taken[*s]).expect("a free slot");
                taken[s] = true;
                s as u32
            })
        })
        .collect()
}

/// The whole program's families. One value for every module of a compile;
/// its digest is in every module's object-cache key.
#[derive(Debug, Default, Clone)]
pub struct ProgramLayouts {
    /// By class id.
    pub families: BTreeMap<u32, FamilyLayout>,
    pub digest: u64,
    /// Keys over all family layouts, and the slots they occupy.
    pub key_total: u32,
    pub slot_total: u32,
    /// Coloured key -> (mask bit, slot).
    pub colored: BTreeMap<String, (u32, u32)>,
    /// Mask words a record carries (0 when nothing is coloured).
    pub mask_words: u32,
    /// Candidate layouts and keys read (colouring census).
    pub layout_count: u32,
    pub read_keys: u32,
    /// Family keys placed at their colour slot, of the coloured family keys.
    pub coloured_placed: u32,
    pub coloured_family_keys: u32,
    /// Late keys (slots past the constructor keys) over all families.
    pub late_total: u32,
    /// Creation-split layouts over all families, and their slots.
    pub variant_total: u32,
    pub variant_slot_total: u32,
}

impl ProgramLayouts {
    /// Does class `class_id`'s family layout put a key at a slot other than
    /// its constructor-order position (so codegen's class slot is not the
    /// instance's)?
    pub fn slots_moved(&self, class_id: u32) -> bool {
        self.families.get(&class_id).is_some_and(|f| {
            f.keys[..f.own_count as usize]
                .iter()
                .enumerate()
                .any(|(i, (_, s))| *s != i as u32)
        })
    }

    /// The (mask bit, slot) of a coloured key.
    pub fn colored(&self, key: &str) -> Option<(u32, u32)> {
        self.colored.get(key).copied()
    }

    /// One line for `PERRY_LAYOUT_REPORT`.
    pub fn report(&self) -> String {
        format!(
            "perry: layout families {} (keys {}, slots {}, late {}, variants {} with {} slots); key colouring: {} layouts, {} keys read, {} coloured; family keys at their colour {}/{}",
            self.families.len(),
            self.key_total,
            self.slot_total,
            self.late_total,
            self.variant_total,
            self.variant_slot_total,
            self.layout_count,
            self.read_keys,
            self.colored.len(),
            self.coloured_placed,
            self.coloured_family_keys
        )
    }
}

/// `PERRY_LAYOUT_REPORT=2`: say why each class is not a family.
fn explain() -> bool {
    std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2")
}

/// Is the family analysis switched off (`PERRY_LAYOUT_FAMILIES=0`)?
fn families_disabled() -> bool {
    std::env::var("PERRY_LAYOUT_FAMILIES").as_deref() == Ok("0")
}

/// Is the colouring switched off (`PERRY_LAYOUT_READS=0`)?
fn colouring_disabled() -> bool {
    std::env::var("PERRY_LAYOUT_READS").as_deref() == Ok("0")
}

/// Mask bits a layout record carries (two words); the top bit is reserved.
pub const MAX_COLOURED_KEYS: usize = 127;

/// Visit every statement and expression of `body`, closures included.
fn visit_stmts<'a>(
    body: &'a [Stmt],
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for s in body {
        visit_stmt(s, on_stmt, on_expr);
    }
}

fn visit_stmt<'a>(
    s: &'a Stmt,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_stmt(s);
    match s {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                visit_expr(e, on_stmt, on_expr);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => visit_expr(e, on_stmt, on_expr),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, on_stmt, on_expr);
            visit_stmts(then_branch, on_stmt, on_expr);
            if let Some(b) = else_branch {
                visit_stmts(b, on_stmt, on_expr);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            visit_expr(condition, on_stmt, on_expr);
            visit_stmts(body, on_stmt, on_expr);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt(i, on_stmt, on_expr);
            }
            if let Some(c) = condition {
                visit_expr(c, on_stmt, on_expr);
            }
            if let Some(u) = update {
                visit_expr(u, on_stmt, on_expr);
            }
            visit_stmts(body, on_stmt, on_expr);
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, on_stmt, on_expr),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, on_stmt, on_expr);
            if let Some(c) = catch {
                visit_stmts(&c.body, on_stmt, on_expr);
            }
            if let Some(f) = finally {
                visit_stmts(f, on_stmt, on_expr);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, on_stmt, on_expr);
            for c in cases {
                if let Some(t) = &c.test {
                    visit_expr(t, on_stmt, on_expr);
                }
                visit_stmts(&c.body, on_stmt, on_expr);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn visit_expr<'a>(
    e: &'a Expr,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_expr(e);
    if let Expr::Closure { body, .. } = e {
        visit_stmts(body, on_stmt, on_expr);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, on_stmt, on_expr));
}

/// Every function body of a module: init, functions, class members.
fn module_bodies(hir: &HirModule) -> Vec<&[Stmt]> {
    let mut out: Vec<&[Stmt]> = vec![hir.init.as_slice()];
    for f in &hir.functions {
        out.push(&f.body);
    }
    for c in &hir.classes {
        for f in c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
        {
            out.push(&f.body);
        }
        for (_, f) in c.getters.iter().chain(c.setters.iter()) {
            out.push(&f.body);
        }
    }
    out
}

/// Can `e` (a store's right-hand side in a constructor prologue) observe the
/// half-built instance? The same whitelist as the codegen prologue analysis
/// (`lower_call::field_init`): parameters, literals and operators over them.
fn inert(e: &Expr, params: &BTreeSet<u32>) -> bool {
    match e {
        Expr::LocalGet(id) => params.contains(id),
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_) => true,
        Expr::Unary { operand, .. } => inert(operand, params),
        Expr::Void(operand) => inert(operand, params),
        _ => false,
    }
}

/// Does `e` (closures included) mention the receiver: `this`, or a `super`
/// form, which reads it implicitly?
fn mentions_receiver(e: &Expr) -> bool {
    let mut found = false;
    visit_expr(e, &mut |_| {}, &mut |x| {
        if matches!(
            x,
            Expr::This
                | Expr::SuperCall(_)
                | Expr::SuperCallSpread(_)
                | Expr::SuperMethodCall { .. }
                | Expr::SuperMethodCallSpread { .. }
                | Expr::SuperPropertyGet { .. }
                | Expr::SuperPropertySet { .. }
        ) {
            found = true;
        }
    });
    found
}

/// Is `s` an expression statement that cannot reach the receiver (it never
/// names it, so it cannot observe or store into the half-built instance)?
fn receiver_free(s: &Stmt) -> bool {
    matches!(s, Stmt::Expr(e) if !mentions_receiver(e))
}

/// The key a prologue statement stores, if it is one.
/// The key a prologue statement stores, if it is one, and whether its value
/// is inert ([`inert`]).
fn prologue_store<'a>(s: &'a Stmt, params: &BTreeSet<u32>) -> Option<(&'a str, bool)> {
    match s {
        Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) if matches!(object.as_ref(), Expr::This) => {
            Some((property.as_str(), inert(value, params)))
        }
        Stmt::Expr(Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        }) if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            match key.as_ref() {
                Expr::String(k) => Some((k.as_str(), inert(value, params))),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A class's own constructor keys, in store order, and whether it has
/// heritage. `None`: not a family member.
fn own_keys(class: &Class, has_parent: bool) -> Result<OwnKeys, String> {
    if class.native_extends.is_some()
        || !class.decorators.is_empty()
        || class.is_literal_shape()
        || class.name.starts_with("__AnonShape_")
    {
        return Err("native heritage, decorators or literal shape".into());
    }
    // No declared fields: a declared field is defined before (base) or right
    // after `super()` (derived) by the field-initialisation phase, which this
    // analysis does not model.
    if class.fields.iter().any(|f| {
        f.origin != ClassFieldOrigin::ConstructorStore
            || f.init.is_some()
            || f.key_expr.is_some()
            || f.is_private
            || !f.decorators.is_empty()
    }) {
        return Err("declared field".into());
    }
    let Some(ctor) = class.constructor.as_ref() else {
        // A base class without a constructor stores nothing; a derived one
        // forwards to its parent.
        return Ok(OwnKeys {
            keys: Vec::new(),
            super_inert: true,
        });
    };
    if ctor.params.iter().any(|p| !p.decorators.is_empty()) {
        return Err("constructor parameters".into());
    }
    // A default runs code before the body: no store of this constructor is
    // an unchecked initialization then.
    let mut open = !ctor.params.iter().any(|p| p.default.is_some());
    let params: BTreeSet<u32> = ctor.params.iter().map(|p| p.id).collect();
    let mut body = ctor.body.as_slice();
    let mut super_inert = true;
    if has_parent {
        while body.first().is_some_and(receiver_free) {
            body = &body[1..];
        }
        match body.first() {
            Some(Stmt::Expr(Expr::SuperCall(args))) => {
                super_inert = open && args.iter().all(|a| inert(a, &params));
                body = &body[1..];
            }
            Some(Stmt::Expr(Expr::SuperCallSpread(_))) => {
                super_inert = false;
                body = &body[1..];
            }
            _ => return Err("derived constructor does not open with super(...)".into()),
        }
    }
    let mut keys: Vec<(String, bool)> = Vec::new();
    for s in body {
        // A statement that never names the receiver (the class-capture
        // reloads the compiler puts first, a static counter bump) cannot
        // observe the instance or skip a later store.
        if receiver_free(s) {
            continue;
        }
        let Some((k, value_inert)) = prologue_store(s, &params) else {
            return Err(format!(
                "constructor statement is not a this.k store: {:.300}",
                format!("{s:?}")
            ));
        };
        if k == "__proto__" || k == "constructor" {
            return Err("reserved key".into());
        }
        // The store itself runs after its value: a value that may run code
        // ends the initialization at this key.
        open &= value_inert;
        if !keys.iter().any(|(x, _)| x == k) {
            keys.push((k.to_string(), open));
        }
    }
    Ok(OwnKeys { keys, super_inert })
}

/// [`own_keys`]: a constructor's keys in store order, each with whether it
/// is stored before any code of the constructor could run (an inert
/// prologue), and whether its `super(...)` call (if any) is inert.
struct OwnKeys {
    keys: Vec<(String, bool)>,
    super_inert: bool,
}

/// The class a binding initialised with a class value names (`var X = class
/// {...}` at any depth): local id -> class name.
fn class_bindings(hir: &HirModule) -> HashMap<u32, String> {
    fn class_of(e: &Expr) -> Option<String> {
        match e {
            Expr::ClassRef(n) => Some(n.clone()),
            Expr::ClassExprFresh { template, .. } => Some(template.clone()),
            // A class expression's evaluation sequence: one class evaluated
            // into a temporary, then statics, then the temporary.
            Expr::Sequence(items) => {
                let mut found = items.iter().filter_map(|i| match i {
                    Expr::LocalSet(_, v) => class_of(v),
                    other => class_of(other),
                });
                let first = found.next()?;
                found.next().is_none().then_some(first)
            }
            _ => None,
        }
    }
    // A binding written with two different classes names neither.
    let mut out: HashMap<u32, Option<String>> = HashMap::new();
    let mut note = |id: u32, name: String| {
        out.entry(id)
            .and_modify(|v| {
                if v.as_deref() != Some(name.as_str()) {
                    *v = None;
                }
            })
            .or_insert(Some(name));
    };
    for body in module_bodies(hir) {
        let mut lets: Vec<(u32, String)> = Vec::new();
        let mut sets: Vec<(u32, String)> = Vec::new();
        visit_stmts(
            body,
            &mut |s| {
                if let Stmt::Let {
                    id,
                    init: Some(init),
                    ..
                } = s
                {
                    // `var` hoisting declares the binding undefined first.
                    if let Some(n) = class_of(init) {
                        lets.push((*id, n));
                    }
                }
            },
            &mut |e| {
                if let Expr::LocalSet(id, value) = e {
                    if let Some(n) = class_of(value) {
                        sets.push((*id, n));
                    }
                }
            },
        );
        for (id, n) in lets.into_iter().chain(sets) {
            note(id, n);
        }
    }
    out.into_iter()
        .filter_map(|(id, n)| Some((id, n?)))
        .collect()
}

/// A static family before its late keys: the class, its runtime parent, its
/// constructor keys in store order, the names its prototype chain defines
/// (methods and accessors: a store of one may run a setter or shadow it, so
/// it never gets a late slot), and the chain's class ids, leaf first.
#[derive(Debug, Clone)]
pub(crate) struct RawFamily {
    pub class_id: u32,
    pub parent: u32,
    pub keys: Vec<String>,
    pub members: BTreeSet<String>,
    pub chain: Vec<u32>,
    /// [`FamilyLayout::init_count`].
    pub init: usize,
    /// A root family's constructor keys stored straight from a parameter:
    /// (parameter index, key). The candidates for its creation split
    /// (`late::add_variants`).
    pub params: Vec<(u32, String)>,
    /// The creation split (`late::add_variants`): the parameter the birth
    /// reads, and per constant the variant's keys in key order.
    pub discriminant: u32,
    pub variants: Vec<(f64, Vec<String>)>,
}

/// A root class's constructor keys stored straight from a parameter, as
/// (parameter index, key).
fn param_keys(class: &Class) -> Vec<(u32, String)> {
    let Some(ctor) = class.constructor.as_ref() else {
        return Vec::new();
    };
    let index: HashMap<u32, u32> = ctor
        .params
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_rest && p.arguments_object.is_none())
        .map(|(i, p)| (p.id, i as u32))
        .collect();
    let mut out = Vec::new();
    for s in &ctor.body {
        let (object, key, value) = match s {
            Stmt::Expr(Expr::PropertySet {
                object,
                property,
                value,
            }) => (object.as_ref(), property.as_str(), value.as_ref()),
            Stmt::Expr(Expr::PutValueSet {
                target, key, value, ..
            }) => match key.as_ref() {
                Expr::String(k) => (target.as_ref(), k.as_str(), value.as_ref()),
                _ => continue,
            },
            _ => continue,
        };
        if let (Expr::This, Expr::LocalGet(id)) = (object, value) {
            if let Some(&i) = index.get(id) {
                out.push((i, key.to_string()));
            }
        }
    }
    out
}

/// The module's classes that are static families.
fn module_families(hir: &HirModule) -> Vec<RawFamily> {
    let mut by_name: HashMap<&str, Vec<&Class>> = HashMap::new();
    for c in &hir.classes {
        by_name.entry(c.name.as_str()).or_default().push(c);
    }
    let bindings = class_bindings(hir);
    let parent_of = |c: &Class| -> Result<Option<&Class>, ()> {
        let name = if let Some(n) = c.extends_name.as_deref() {
            Some(n.to_string())
        } else if let Some(e) = c.extends_expr.as_deref() {
            match e {
                Expr::LocalGet(id) => Some(bindings.get(id).cloned().ok_or(())?),
                Expr::ClassRef(n) => Some(n.clone()),
                _ => return Err(()),
            }
        } else {
            None
        };
        match name {
            None => Ok(None),
            Some(n) => match by_name.get(n.as_str()) {
                Some(v) if v.len() == 1 => Ok(Some(v[0])),
                _ => Err(()),
            },
        }
    };
    let accessor = |c: &Class, k: &str| {
        c.getters.iter().any(|(n, _)| n == k) || c.setters.iter().any(|(n, _)| n == k)
    };
    let mut out = Vec::new();
    'class: for c in &hir.classes {
        // The chain, leaf first.
        let mut chain: Vec<&Class> = vec![c];
        let mut cur = c;
        loop {
            match parent_of(cur) {
                Err(()) => {
                    if explain() {
                        let writes = std::cell::RefCell::new(Vec::<String>::new());
                        if let Some(Expr::LocalGet(want)) = cur.extends_expr.as_deref() {
                            for body in module_bodies(hir) {
                                visit_stmts(
                                    body,
                                    &mut |s| {
                                        if let Stmt::Let { id, init, .. } = s {
                                            if id == want {
                                                writes.borrow_mut().push(format!(
                                                    "let {:.160}",
                                                    format!("{init:?}")
                                                ));
                                            }
                                        }
                                    },
                                    &mut |e| {
                                        if let Expr::LocalSet(id, v) = e {
                                            if id == want {
                                                writes
                                                    .borrow_mut()
                                                    .push(format!("set {:.160}", format!("{v:?}")));
                                            }
                                        }
                                    },
                                );
                            }
                        }
                        eprintln!(
                            "perry: layout family refused: {}: unresolved parent {:?} {:.300} writes {:?}",
                            c.name,
                            cur.extends_name,
                            format!("{:?}", cur.extends_expr),
                            writes.borrow()
                        );
                    }
                    continue 'class;
                }
                Ok(None) => break,
                Ok(Some(p)) => {
                    if chain.len() > 32 || chain.iter().any(|x| std::ptr::eq(*x, p)) {
                        continue 'class;
                    }
                    chain.push(p);
                    cur = p;
                }
            }
        }
        let mut keys: Vec<String> = Vec::new();
        // Evaluation order: every `super(...)`'s arguments (leaf first), then
        // the constructor bodies from the root. The initialization is open
        // while nothing that may run code has been evaluated since the birth.
        let mut owns = Vec::with_capacity(chain.len());
        for (depth, k) in chain.iter().enumerate().rev() {
            let own = match own_keys(k, depth + 1 < chain.len()) {
                Ok(own) => own,
                Err(why) => {
                    if explain() {
                        eprintln!(
                            "perry: layout family refused: {} ({}): {why}",
                            c.name, k.name
                        );
                    }
                    continue 'class;
                }
            };
            owns.push(own);
        }
        let mut open = owns.iter().all(|o| o.super_inert);
        let mut init = 0usize;
        for own in owns {
            for (key, eligible) in own.keys {
                open &= eligible;
                if !keys.contains(&key) {
                    keys.push(key);
                    if open {
                        init = keys.len();
                    }
                }
            }
        }
        if keys.is_empty() || keys.iter().any(|k| chain.iter().any(|x| accessor(x, k))) {
            continue;
        }
        let parent = chain.get(1).map_or(0, |p| p.id);
        if explain() {
            eprintln!(
                "perry: layout family: {} (class {}): {}",
                c.name,
                c.id,
                keys.join(",")
            );
        }
        let mut members = BTreeSet::new();
        for x in &chain {
            members.extend(x.methods.iter().map(|m| m.name.clone()));
            members.extend(x.getters.iter().map(|(n, _)| n.clone()));
            members.extend(x.setters.iter().map(|(n, _)| n.clone()));
        }
        out.push(RawFamily {
            class_id: c.id,
            parent,
            keys,
            members,
            chain: chain.iter().map(|x| x.id).collect(),
            init,
            params: if chain.len() == 1 {
                param_keys(c)
            } else {
                Vec::new()
            },
            discriminant: 0,
            variants: Vec::new(),
        });
    }
    out
}

mod late;
use late::{add_late_keys, add_variants, late_budget, module_groups, Group};

/// The `this.k = ...` keys a function body stores, in order (closures not
/// entered: their `this` is another binding unless an arrow, and an arrow's
/// stores follow the body's own anyway).
fn this_store_keys(body: &[Stmt]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    fn walk(s: &Stmt, keys: &mut Vec<String>) {
        let mut note = |k: &str| {
            if !keys.iter().any(|x| x == k) {
                keys.push(k.to_string());
            }
        };
        match s {
            Stmt::Expr(Expr::PropertySet {
                object, property, ..
            }) if matches!(object.as_ref(), Expr::This) => note(property),
            Stmt::Expr(Expr::PutValueSet { target, key, .. })
                if matches!(target.as_ref(), Expr::This) =>
            {
                if let Expr::String(k) = key.as_ref() {
                    note(k)
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for t in then_branch {
                    walk(t, keys);
                }
                if let Some(b) = else_branch {
                    for t in b {
                        walk(t, keys);
                    }
                }
            }
            _ => {}
        }
    }
    for s in body {
        walk(s, &mut keys);
    }
    keys
}

/// A class instance's keys in birth order: the chain's fields, ancestors
/// first, then the constructor's own stores.
fn class_keys(c: &Class, by_id: &HashMap<u32, &Class>) -> Vec<String> {
    let mut chain: Vec<&Class> = vec![c];
    let mut cur = c;
    while let Some(p) = cur.extends.and_then(|id| by_id.get(&id).copied()) {
        if chain.len() > 32 || chain.iter().any(|x| std::ptr::eq(*x, p)) {
            break;
        }
        chain.push(p);
        cur = p;
    }
    let mut keys: Vec<String> = Vec::new();
    for k in chain.iter().rev() {
        for f in &k.fields {
            if f.is_private || f.key_expr.is_some() {
                continue;
            }
            keys.push(f.name.clone());
        }
        if let Some(ctor) = k.constructor.as_ref() {
            for s in this_store_keys(&ctor.body) {
                if !keys.contains(&s) {
                    keys.push(s);
                }
            }
        }
    }
    keys
}

fn colouring(modules: &[&HirModule]) -> ProgramLayouts {
    if colouring_disabled() {
        return ProgramLayouts::default();
    }
    let mut reads: HashMap<String, u64> = HashMap::new();
    let mut layouts: Vec<Vec<String>> = Vec::new();
    let mut by_id: HashMap<u32, &Class> = HashMap::new();
    for hir in modules {
        for c in &hir.classes {
            by_id.insert(c.id, c);
        }
    }
    for hir in modules {
        for body in module_bodies(hir) {
            visit_stmts(body, &mut |_| {}, &mut |e| match e {
                Expr::PropertyGet { property, .. } => {
                    *reads.entry(property.clone()).or_default() += 1;
                }
                Expr::Object(props) => {
                    let mut keys: Vec<String> = Vec::new();
                    for (k, _) in props {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                    layouts.push(keys);
                }
                Expr::Closure { body, .. } => {
                    let keys = this_store_keys(body);
                    if !keys.is_empty() {
                        layouts.push(keys);
                    }
                }
                _ => {}
            });
        }
        for f in &hir.functions {
            let keys = this_store_keys(&f.body);
            if !keys.is_empty() {
                layouts.push(keys);
            }
        }
        for c in &hir.classes {
            let keys = class_keys(c, &by_id);
            if !keys.is_empty() {
                layouts.push(keys);
            }
        }
    }
    // Per key: how many layouts hold it at each position.
    let mut positions: HashMap<&str, BTreeMap<u32, u32>> = HashMap::new();
    for l in &layouts {
        for (pos, k) in l.iter().enumerate() {
            // A key a layout holds twice (a redeclared field) has no one
            // position there.
            if l.iter().filter(|x| *x == k).count() != 1 || !reads.contains_key(k) {
                continue;
            }
            if k.is_empty() || k.len() > 64 || k == "__proto__" || k == "length" || k == "size" {
                continue;
            }
            *positions
                .entry(k.as_str())
                .or_default()
                .entry(pos as u32)
                .or_default() += 1;
        }
    }
    let mut candidates: Vec<(&str, u64)> = positions
        .keys()
        .map(|k| (*k, reads.get(*k).copied().unwrap_or(0)))
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    candidates.truncate(MAX_COLOURED_KEYS);
    let mut out = ProgramLayouts {
        layout_count: layouts.len() as u32,
        read_keys: reads.len() as u32,
        ..Default::default()
    };
    let mut chosen: Vec<(&str, u32)> = candidates
        .iter()
        .map(|(k, _)| {
            let slot = positions[k]
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(p, _)| *p)
                .unwrap_or(0);
            (*k, slot)
        })
        .collect();
    chosen.sort();
    for (bit, (k, slot)) in chosen.iter().enumerate() {
        out.colored.insert((*k).to_string(), (bit as u32, *slot));
    }
    out.mask_words = if out.colored.is_empty() { 0 } else { 2 };
    if explain() {
        for (k, (bit, slot)) in &out.colored {
            eprintln!("perry: colour {k} bit {bit} slot {slot} reads {}", reads[k]);
        }
    }
    out
}

/// The slot of every key of every family layout, in key order. Slots are a
/// permutation of the layout's key positions and agree along a parent chain:
/// an inherited key takes its parent's slot. A coloured key takes its colour
/// slot when that is free and inside the layout; every other key the lowest
/// free slot.
pub fn assign_slots(
    raw: &[(u32, u32, Vec<String>, usize)],
    colored: &BTreeMap<String, (u32, u32)>,
) -> Vec<Vec<u32>> {
    let by_class: HashMap<u32, usize> = raw
        .iter()
        .enumerate()
        .map(|(i, (c, _, _, _))| (*c, i))
        .collect();
    let mut done: Vec<Option<Vec<u32>>> = vec![None; raw.len()];
    fn assign(
        i: usize,
        raw: &[(u32, u32, Vec<String>, usize)],
        by_class: &HashMap<u32, usize>,
        colored: &BTreeMap<String, (u32, u32)>,
        done: &mut Vec<Option<Vec<u32>>>,
        depth: u32,
    ) -> Vec<u32> {
        if let Some(s) = &done[i] {
            return s.clone();
        }
        let (_, parent, keys, own) = &raw[i];
        let (n, own) = (keys.len(), *own);
        // Two tiers: the constructor keys take slots `0..own` (the birth key
        // list's permutation), the late keys `own..n`.
        let tier = |pos: usize| if pos < own { 0..own } else { own..n };
        let mut slots: Vec<Option<u32>> = vec![None; n];
        let mut taken = vec![false; n];
        // Inherited keys: the parent's slots where they fall in the key's
        // tier (a subclass's constructor keys extend its parent's, so the
        // parent's constructor slots agree; its late keys follow).
        if let Some(&p) = by_class.get(parent).filter(|_| depth < 32) {
            let ps = assign(p, raw, by_class, colored, done, depth + 1);
            for (pk, s) in raw[p].2.iter().zip(ps) {
                if let Some(pos) = keys.iter().position(|k| k == pk) {
                    if tier(pos).contains(&(s as usize))
                        && !taken[s as usize]
                        && slots[pos].is_none()
                    {
                        slots[pos] = Some(s);
                        taken[s as usize] = true;
                    }
                }
            }
        }
        for (pos, k) in keys.iter().enumerate() {
            if slots[pos].is_some() {
                continue;
            }
            if let Some(&(_, c)) = colored.get(k) {
                if tier(pos).contains(&(c as usize)) && !taken[c as usize] {
                    slots[pos] = Some(c);
                    taken[c as usize] = true;
                }
            }
        }
        let mut out: Vec<u32> = Vec::with_capacity(n);
        for (pos, s) in slots.into_iter().enumerate() {
            let s = s.unwrap_or_else(|| {
                let s = tier(pos)
                    .find(|s| !taken[*s])
                    .expect("a tier has a free slot for each of its keys");
                taken[s] = true;
                s as u32
            });
            out.push(s);
        }
        done[i] = Some(out.clone());
        out
    }
    (0..raw.len())
        .map(|i| assign(i, raw, &by_class, colored, &mut done, 0))
        .collect()
}

/// Build the program's layouts from the families' keys and the colouring.
fn layouts_of(
    raw: Vec<(u32, u32, Vec<String>, usize)>,
    mut variants: HashMap<u32, (u32, Vec<(f64, Vec<String>)>)>,
    mut layouts: ProgramLayouts,
) -> ProgramLayouts {
    let slots = assign_slots(&raw, &layouts.colored);
    for ((class_id, parent_class_id, keys, own), slots) in raw.into_iter().zip(slots) {
        let slot_count = keys.len() as u32;
        layouts.key_total += keys.len() as u32;
        layouts.slot_total += slot_count;
        for (k, s) in keys.iter().zip(&slots) {
            if let Some(&(_, c)) = layouts.colored.get(k) {
                layouts.coloured_family_keys += 1;
                layouts.coloured_placed += u32::from(c == *s);
            }
        }
        if explain() {
            let shown: Vec<String> = keys
                .iter()
                .zip(&slots)
                .map(|(k, s)| format!("{k}@{s}"))
                .collect();
            eprintln!(
                "perry: layout family slots: class {class_id}: {}",
                shown.join(",")
            );
        }
        let keys: Vec<(String, u32)> = keys.into_iter().zip(slots).collect();
        let (discriminant, split) = variants.remove(&class_id).unwrap_or_default();
        let split: Vec<FamilyVariant> = split
            .into_iter()
            .map(|(k, vkeys)| {
                let vslots = variant_slots(&vkeys, own, &keys, &layouts.colored);
                layouts.variant_total += 1;
                layouts.variant_slot_total += vkeys.len() as u32;
                if explain() {
                    let shown: Vec<String> = vkeys
                        .iter()
                        .zip(&vslots)
                        .map(|(k, s)| format!("{k}@{s}"))
                        .collect();
                    eprintln!(
                        "perry: layout family variant: class {class_id} arg{discriminant}={k}: {}",
                        shown.join(",")
                    );
                }
                FamilyVariant {
                    key_bits: k.to_bits(),
                    slot_count: vkeys.len() as u32,
                    keys: vkeys.into_iter().zip(vslots).collect(),
                }
            })
            .collect();
        layouts.families.insert(
            class_id,
            FamilyLayout {
                class_id,
                parent_class_id,
                keys,
                slot_count,
                own_count: own as u32,
                init_count: own as u32,
                discriminant,
                variants: split,
            },
        );
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in layouts.families.values() {
        f.hash(&mut h);
    }
    layouts.colored.hash(&mut h);
    layouts.digest = h.finish();
    layouts
}

/// The program's families and colouring over every module's final HIR.
pub fn compute<'a>(
    modules: impl IntoIterator<Item = &'a HirModule>,
    families: bool,
) -> ProgramLayouts {
    let modules: Vec<&HirModule> = modules.into_iter().collect();
    let colours = colouring(&modules);
    let mut fams: Vec<RawFamily> = Vec::new();
    if families && !families_disabled() {
        for hir in &modules {
            fams.extend(module_families(hir));
        }
    }
    let owns: Vec<usize> = fams.iter().map(|f| f.keys.len()).collect();
    let late = if fams.is_empty() || late_budget() == 0 {
        0
    } else {
        let before: usize = owns.iter().sum();
        let groups: Vec<(Option<u32>, Group)> =
            modules.iter().flat_map(|hir| module_groups(hir)).collect();
        add_variants(&mut fams, &groups);
        add_late_keys(&mut fams, &groups);
        fams.iter().map(|f| f.keys.len()).sum::<usize>() - before
    };
    let variants: HashMap<u32, (u32, Vec<(f64, Vec<String>)>)> = fams
        .iter_mut()
        .filter(|f| !f.variants.is_empty())
        .map(|f| {
            (
                f.class_id,
                (f.discriminant, std::mem::take(&mut f.variants)),
            )
        })
        .collect();
    let inits: HashMap<u32, u32> = fams.iter().map(|f| (f.class_id, f.init as u32)).collect();
    let raw: Vec<(u32, u32, Vec<String>, usize)> = fams
        .into_iter()
        .zip(owns)
        .map(|(f, own)| (f.class_id, f.parent, f.keys, own))
        .collect();
    let mut layouts = layouts_of(raw, variants, colours);
    for (id, init) in inits {
        if let Some(f) = layouts.families.get_mut(&id) {
            f.init_count = init.min(f.own_count);
        }
    }
    layouts.late_total = late as u32;
    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some() {
        eprintln!("{}", layouts.report());
    }
    layouts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_subclass_agrees_with_its_base_on_inherited_slots() {
        let raw = vec![
            (
                1,
                0,
                keys(&[
                    "pos",
                    "end",
                    "kind",
                    "id",
                    "flags",
                    "transformFlags",
                    "parent",
                    "emitNode",
                ]),
                8,
            ),
            (
                2,
                1,
                keys(&[
                    "pos",
                    "end",
                    "kind",
                    "id",
                    "flags",
                    "transformFlags",
                    "parent",
                    "emitNode",
                    "escapedText",
                ]),
                9,
            ),
        ];
        let l = layouts_of(raw, HashMap::new(), ProgramLayouts::default());
        let base = &l.families[&1];
        let sub = &l.families[&2];
        for (k, s) in &base.keys {
            assert_eq!(sub.keys.iter().find(|(x, _)| x == k).unwrap().1, *s, "{k}");
        }
        assert_eq!(sub.slot_count, 9);
        assert!(!l.slots_moved(2));
    }

    #[test]
    fn a_coloured_key_takes_its_free_colour_slot() {
        let raw = vec![
            (1, 0, keys(&["a", "b", "kind"]), 3),
            (2, 1, keys(&["a", "b", "kind", "x", "parent"]), 5),
        ];
        let mut colours = ProgramLayouts::default();
        colours.colored.insert("kind".into(), (0, 0));
        colours.colored.insert("parent".into(), (1, 3));
        let l = layouts_of(raw, HashMap::new(), colours);
        let base = &l.families[&1];
        assert_eq!(
            base.keys,
            vec![("a".into(), 1), ("b".into(), 2), ("kind".into(), 0)]
        );
        let sub = &l.families[&2];
        // Inherited slots agree; `parent` takes its colour 3, `x` slot 4.
        assert_eq!(
            sub.keys,
            vec![
                ("a".into(), 1),
                ("b".into(), 2),
                ("kind".into(), 0),
                ("x".into(), 4),
                ("parent".into(), 3)
            ]
        );
    }

    #[test]
    fn late_keys_take_the_slots_after_the_constructor_keys() {
        let raw = vec![
            (1, 0, keys(&["a", "kind", "late1", "late2"]), 2),
            (
                2,
                1,
                keys(&["a", "kind", "b", "late1", "late2", "late3"]),
                3,
            ),
        ];
        let mut colours = ProgramLayouts::default();
        colours.colored.insert("kind".into(), (0, 0));
        colours.colored.insert("late2".into(), (1, 2));
        let l = layouts_of(raw, HashMap::new(), colours);
        let base = &l.families[&1];
        assert_eq!(
            base.keys,
            vec![
                ("a".into(), 1),
                ("kind".into(), 0),
                ("late1".into(), 3),
                ("late2".into(), 2)
            ]
        );
        assert_eq!(base.own_count, 2);
        let sub = &l.families[&2];
        // `b` cannot keep the parent's late slot 2: constructor keys stay in
        // 0..3 and late keys in 3..6, so `late2` moves off its colour.
        assert_eq!(sub.own_count, 3);
        for (k, s) in &sub.keys[..3] {
            assert!(*s < 3, "{k}");
        }
        for (k, s) in &sub.keys[3..] {
            assert!(*s >= 3, "{k}");
        }
    }
}

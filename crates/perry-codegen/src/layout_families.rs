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

#[path = "layout_families/literal.rs"]
mod literal;

/// One key of a family layout. Only a [`FamilyKey::Name`] is a property: it
/// alone compares equal to a property name (`== "x"`), so no consumer that
/// looks a property up by name can reach a private element.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FamilyKey {
    /// A string-named property.
    Name(String),
    /// A private field of the instance (#11791): an `ENTRY_PRIVATE` element,
    /// never a property, stored under the runtime's qualified private name
    /// (`codegen::static_private_class::private_storage_key`). Its
    /// definition is the runtime's claim (`js_private_field_add`), which
    /// fills the slot's hole.
    Private(String),
}

impl FamilyKey {
    /// The property name, when the key is a property.
    pub fn name(&self) -> Option<&str> {
        match self {
            FamilyKey::Name(n) => Some(n),
            FamilyKey::Private(_) => None,
        }
    }

    /// The key's atom spelling: the property name, or the private field's
    /// qualified storage name.
    pub fn spelling(&self) -> &str {
        match self {
            FamilyKey::Name(n) | FamilyKey::Private(n) => n,
        }
    }

    pub fn is_private(&self) -> bool {
        matches!(self, FamilyKey::Private(_))
    }
}

impl PartialEq<str> for FamilyKey {
    fn eq(&self, other: &str) -> bool {
        self.name() == Some(other)
    }
}

impl PartialEq<&str> for FamilyKey {
    fn eq(&self, other: &&str) -> bool {
        self.name() == Some(*other)
    }
}

impl PartialEq<String> for FamilyKey {
    fn eq(&self, other: &String) -> bool {
        self.name() == Some(other.as_str())
    }
}

impl From<&str> for FamilyKey {
    fn from(name: &str) -> Self {
        FamilyKey::Name(name.to_string())
    }
}

impl From<String> for FamilyKey {
    fn from(name: String) -> Self {
        FamilyKey::Name(name)
    }
}

impl std::fmt::Display for FamilyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.spelling())
    }
}

/// One static class family's layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FamilyLayout {
    pub class_id: u32,
    /// The runtime parent class id this layout assumes (0: no heritage).
    pub parent_class_id: u32,
    /// The keys in enumeration (store) order with their slots
    /// ([`assign_slots`]).
    pub keys: Vec<(FamilyKey, u32)>,
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
    pub keys: Vec<(FamilyKey, u32)>,
    pub slot_count: u32,
}

/// The slots of a variant's keys: the constructor keys (`keys[..own]`, the
/// same as the default layout's) at the default's slots; a late key at its
/// slot in the default layout or its colour slot where free and inside the
/// variant; every other key the lowest free slot.
fn variant_slots(
    keys: &[FamilyKey],
    own: usize,
    default: &[(FamilyKey, u32)],
    colored: &BTreeMap<String, (u32, u32)>,
) -> Vec<u32> {
    let n = keys.len();
    let mut taken = vec![false; n];
    let mut slots: Vec<Option<u32>> = vec![None; n];
    let at = |k: &FamilyKey| default.iter().find(|(d, _)| d == k).map(|(_, s)| *s);
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
                k.name().and_then(|n| colored.get(n)).map(|(_, c)| *c)
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
    /// Structurally run-once, closed-world evaluated declarations. Compile
    /// facts only; generated code adds no flag, table or observation state.
    pub run_once_evaluated: BTreeSet<u32>,
    /// By class id.
    pub families: BTreeMap<u32, FamilyLayout>,
    /// Literal prototype identity and source-ordered base slots; compile facts only.
    pub literal_families: BTreeSet<u32>,
    /// Classes not admitted, counted once each by the first failed proof.
    pub refused: BTreeMap<String, u32>,
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
        let admitted = format!(
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
        );
        format!(
            "{admitted}; refused {}: {}",
            self.refused.values().sum::<u32>(),
            self.refused
                .iter()
                .map(|(why, n)| format!("{why}={n}"))
                .collect::<Vec<_>>()
                .join(", ")
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
    visit_expr(e, &mut |_| {}, &mut |x| found |= names_receiver(x));
    found
}

/// Is `e` itself `this` or a `super` form?
fn names_receiver(e: &Expr) -> bool {
    matches!(
        e,
        Expr::This
            | Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
    )
}

/// Is `s` an expression statement that cannot reach the receiver (it never
/// names it, so it cannot observe or store into the half-built instance)?
fn receiver_free(s: &Stmt) -> bool {
    matches!(s, Stmt::Expr(e) if !mentions_receiver(e))
}

/// A statement before a derived constructor's `super(...)`: the instance
/// does not exist yet, so one that never names the receiver (closures
/// included: an arrow could call `super`) and never returns cannot observe
/// or replace it.
fn before_super_free(s: &Stmt) -> bool {
    let free = std::cell::Cell::new(true);
    visit_stmt(
        s,
        &mut |t| free.set(free.get() && !matches!(t, Stmt::Return(_))),
        &mut |e| free.set(free.get() && !names_receiver(e)),
    );
    free.get()
}

/// A parameter default's fill, `if (p === undefined) p = v` (`p` a
/// parameter, `v` not naming the receiver): the value `v` it may evaluate.
/// It never reaches the instance; it runs code only when `v` does.
fn param_default_fill<'a>(s: &'a Stmt, params: &BTreeSet<u32>) -> Option<&'a Expr> {
    let Stmt::If {
        condition:
            Expr::Compare {
                op: perry_hir::CompareOp::Eq,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = s
    else {
        return None;
    };
    let (Expr::LocalGet(p), Expr::Undefined) = (left.as_ref(), right.as_ref()) else {
        return None;
    };
    match then_branch.as_slice() {
        [Stmt::Expr(Expr::LocalSet(q, v))] if q == p && params.contains(p) => {
            (!mentions_receiver(v)).then_some(v.as_ref())
        }
        _ => None,
    }
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
/// Does `class` extend node's `EventEmitter` by that name (an import from
/// `node:events`, or a binding of it the module does not declare as a
/// class), the heritage `super(opts)` lowers to its `init` for
/// (`expr::lower_event_emitter_subclass_init`)?
fn node_event_emitter_heritage(class: &Class) -> bool {
    class.extends_name.as_deref() == Some("EventEmitter")
        && class.extends_expr.is_none()
        && class
            .native_extends
            .as_ref()
            .is_none_or(|(module, export)| {
                module.trim_start_matches("node:") == "events" && export == "EventEmitter"
            })
}

/// `fresh`: `class` is a per-evaluation template (`ClassExprFresh`), whose
/// evaluations name their private elements per evaluation, never by the
/// template's spelling.
fn own_keys(class: &Class, has_parent: bool, fresh: bool) -> Result<OwnKeys, String> {
    if class.is_literal_shape() {
        return literal::own_keys(class);
    }
    if (class.native_extends.is_some() && !node_event_emitter_heritage(class))
        || !class.decorators.is_empty()
        || class.is_literal_shape()
        || class.name.starts_with("__AnonShape_")
    {
        return Err("native heritage, decorators or literal shape".into());
    }
    // A public field with a plain name is defined before (base) or right
    // after `super()` (derived) by the field-initialisation phase, in source
    // order and ahead of every constructor-body store: a key of the family
    // like a prologue store. A private field is the runtime's claim of its
    // private element (`js_private_field_add`), at the same point. Anything
    // else the phase can do (a computed key, a decorator, a reservation that
    // carries an initializer) is not modelled.
    let mut defined: Vec<(FamilyKey, bool)> = Vec::new();
    let mut defined_open = true;
    for f in &class.fields {
        // A class capture (`__perry_cap_*`) is never defined by the
        // field-initialisation phase (`field_init` skips it): the
        // constructor's own `this.__perry_cap_* = <capture param>` stash
        // stores it, so it is a constructor key like any prologue store.
        if f.origin == ClassFieldOrigin::Definition
            && f.key_expr.is_none()
            && f.init.is_none()
            && !f.is_private
            && f.decorators.is_empty()
            && f.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX)
        {
            continue;
        }
        // A private field: its element joins the layout under its qualified
        // storage name. A brand (private methods or accessors) would move
        // the instance off its layout, and a specialized class's elements
        // carry another class's id.
        if f.origin == ClassFieldOrigin::Definition
            && f.is_private
            && f.key_expr.is_none()
            && f.decorators.is_empty()
            && !class.has_private_instance_brand()
            && class.specialized_from.is_none()
            && !fresh
        {
            defined_open &= f.init.as_ref().is_none_or(|e| inert(e, &BTreeSet::new()));
            let key = FamilyKey::Private(
                crate::codegen::static_private_class::private_storage_key(class.id, &f.name),
            );
            if !defined.iter().any(|(x, _)| *x == key) {
                defined.push((key, defined_open));
            }
            // The claim is a runtime call: no later key is an unchecked
            // initialization.
            defined_open = false;
            continue;
        }
        let plain = f.key_expr.is_none()
            && !f.is_private
            && f.decorators.is_empty()
            && !f.name.starts_with("__perry_cap_");
        match f.origin {
            ClassFieldOrigin::ConstructorStore if plain && f.init.is_none() => {}
            // A declared key (`Class::is_declared_keys_record`): a key of the
            // family that no birth writes, so never an initialization.
            ClassFieldOrigin::Declared if plain && f.init.is_none() => {
                if f.name == "__proto__" || f.name == "constructor" {
                    return Err("reserved key".into());
                }
                defined_open = false;
                if !defined.iter().any(|(x, _)| *x == f.name) {
                    defined.push((FamilyKey::Name(f.name.clone()), false));
                }
            }
            ClassFieldOrigin::Definition if plain => {
                if f.name == "__proto__" || f.name == "constructor" {
                    return Err("reserved key".into());
                }
                // The value is evaluated before the key is defined: a value
                // that may run code ends the initialization at this key.
                defined_open &= f.init.as_ref().is_none_or(|e| inert(e, &BTreeSet::new()));
                if !defined.iter().any(|(x, _)| *x == f.name) {
                    defined.push((FamilyKey::Name(f.name.clone()), defined_open));
                }
            }
            _ => {
                let why = if f.key_expr.is_some() {
                    "computed key"
                } else if f.is_private {
                    "private name (brand, fresh evaluation, specialized class or decorator)"
                } else if !f.decorators.is_empty() {
                    "decorator"
                } else if f.name.starts_with("__perry_cap_") {
                    "class capture"
                } else {
                    "reservation with an initializer"
                };
                return Err(format!("declared field: {why}"));
            }
        }
    }
    let Some(ctor) = class.constructor.as_ref() else {
        // A base class without a constructor stores only its fields; a
        // derived one forwards to its parent.
        return Ok(OwnKeys {
            keys: defined,
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
        while let Some(s) = body.first() {
            if let Some(v) = param_default_fill(s, &params) {
                open &= inert(v, &params);
            } else if !receiver_free(s) {
                if !before_super_free(s) {
                    break;
                }
                // A check that throws, a local computed from the arguments:
                // it may run code.
                open = false;
            }
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
    let mut keys: Vec<(FamilyKey, bool)> = defined
        .into_iter()
        .map(|(k, eligible)| (k, eligible && open))
        .collect();
    open &= defined_open;
    for s in body {
        // A statement that never names the receiver (the class-capture
        // reloads the compiler puts first, a static counter bump) cannot
        // observe the instance or skip a later store.
        if receiver_free(s) {
            continue;
        }
        if let Some(v) = param_default_fill(s, &params) {
            open &= inert(v, &params);
            continue;
        }
        if let Some((k, value_inert)) = prologue_store(s, &params) {
            if k == "__proto__" || k == "constructor" {
                return Err("reserved key".into());
            }
            // The store itself runs after its value: a value that may run
            // code ends the initialization at this key.
            open &= value_inert;
            if !keys.iter().any(|(x, _)| x == k) {
                keys.push((FamilyKey::Name(k.to_string()), open));
            }
            continue;
        }
        // Any other statement may run code: the initialization ends here.
        // The keys it stores itself (a branch, a `switch` case, a loop
        // body; closures not entered) follow in source order, the order an
        // execution adds them in; an execution that skips one leaves its
        // hole, and one that adds keys out of this order gets the object's
        // overflow (the store probe orders it).
        open = false;
        let mut nested: Vec<&str> = Vec::new();
        nested_this_stores(s, &mut nested);
        for k in nested {
            if k == "__proto__" || k == "constructor" {
                return Err("reserved key".into());
            }
            if !keys.iter().any(|(x, _)| x == k) {
                keys.push((FamilyKey::Name(k.to_string()), false));
            }
        }
    }
    Ok(OwnKeys { keys, super_inert })
}

/// The `this.k = v` keys statement `s` stores in its own body, in source
/// order: through blocks, branches, `switch` cases, loop bodies and `try`
/// arms, not into closures (an arrow's stores run whenever it is called).
fn nested_this_stores<'a>(s: &'a Stmt, out: &mut Vec<&'a str>) {
    let all = |body: &'a [Stmt], out: &mut Vec<&'a str>| {
        for t in body {
            nested_this_stores(t, out);
        }
    };
    match s {
        Stmt::Expr(Expr::PropertySet {
            object, property, ..
        }) if matches!(object.as_ref(), Expr::This) => out.push(property.as_str()),
        Stmt::Expr(Expr::PutValueSet {
            target,
            key,
            receiver,
            ..
        }) if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            if let Expr::String(k) = key.as_ref() {
                out.push(k.as_str());
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            all(then_branch, out);
            if let Some(b) = else_branch {
                all(b, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            all(body, out)
        }
        Stmt::Labeled { body, .. } => nested_this_stores(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            all(body, out);
            if let Some(c) = catch {
                all(&c.body, out);
            }
            if let Some(f) = finally {
                all(f, out);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                all(&c.body, out);
            }
        }
        _ => {}
    }
}

/// [`own_keys`]: a constructor's keys in store order, each with whether it
/// is stored before any code of the constructor could run (an inert
/// prologue), and whether its `super(...)` call (if any) is inert.
struct OwnKeys {
    keys: Vec<(FamilyKey, bool)>,
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

/// The classes whose instances the program stores, straight from `new`,
/// into another object's property or an element (`links[id] = new
/// SymbolLinks()`, `symbol.links = new SymbolLinks()`; not `this.x = new
/// C()`, a constructor's own part): records other code fills.
fn filled_records(modules: &[&HirModule]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for hir in modules {
        let bound = class_bindings(hir);
        for body in module_bodies(hir) {
            visit_stmts(body, &mut |_| {}, &mut |e| {
                let value = match e {
                    Expr::PropertySet { object, value, .. }
                        if !matches!(object.as_ref(), Expr::This) =>
                    {
                        value
                    }
                    Expr::PutValueSet { target, value, .. }
                        if !matches!(target.as_ref(), Expr::This) =>
                    {
                        value
                    }
                    Expr::IndexSet { value, .. } => value,
                    _ => return,
                };
                match value.as_ref() {
                    Expr::New { class_name, .. } => {
                        out.insert(class_name.clone());
                    }
                    Expr::NewDynamic { callee, .. } => {
                        if let Expr::LocalGet(l) = callee.as_ref() {
                            out.extend(bound.get(l).cloned());
                        }
                    }
                    _ => {}
                }
            });
        }
    }
    out
}

/// A static family before its late keys: the class, its runtime parent, its
/// constructor keys in store order, the names its prototype chain defines
/// (methods and accessors: a store of one may run a setter or shadow it, so
/// it never gets a late slot), and the chain's class ids, leaf first.
#[derive(Debug, Clone)]
pub(crate) struct RawFamily {
    pub class_id: u32,
    pub parent: u32,
    pub keys: Vec<FamilyKey>,
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
    pub variants: Vec<(f64, Vec<FamilyKey>)>,
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

mod parents;

/// Every class chain is resolved over the whole program, with the owning
/// module retained at every step (local ids and names are module scoped).
fn program_families(
    modules: &[(&str, &HirModule)],
    reexports: &BTreeMap<(String, String), String>,
    refused: &mut BTreeMap<String, u32>,
) -> Vec<RawFamily> {
    let graph = parents::Parents::new(modules, reexports);
    let mut refuse = |c: &Class, why: &str| {
        crate::fallback_diag::family_refused(c.id, why);
        *refused.entry(why.to_string()).or_default() += 1;
        if explain() {
            eprintln!("perry: layout family refused: {}: {why}", c.name);
        }
    };
    let accessor = |c: &Class, k: &str| {
        c.getters.iter().any(|(n, _)| n == k) || c.setters.iter().any(|(n, _)| n == k)
    };
    let fresh: BTreeSet<u32> = modules
        .iter()
        .flat_map(|(_, hir)| {
            let templates = crate::codegen::fresh_class_templates::fresh_class_templates(hir);
            hir.classes
                .iter()
                .filter(move |c| templates.contains(&c.name))
                .map(|c| c.id)
        })
        .collect();
    let mut out = Vec::new();
    'class: for c in modules.iter().flat_map(|(_, hir)| &hir.classes) {
        // The chain, leaf first.
        let mut chain: Vec<&Class> = vec![c];
        let mut cur = c;
        loop {
            match graph.parent(cur) {
                Err(why) => {
                    refuse(c, why);
                    continue 'class;
                }
                Ok(None) => break,
                Ok(Some(p)) => {
                    if chain.len() > 32 || chain.iter().any(|x| std::ptr::eq(*x, p)) {
                        refuse(c, "inheritance-cycle-or-depth");
                        continue 'class;
                    }
                    chain.push(p);
                    cur = p;
                }
            }
        }
        let mut keys: Vec<FamilyKey> = Vec::new();
        // The chain's root may extend an opaque native parent, whose
        // constructor runs code and adds its keys first.
        let native = graph.native(chain[chain.len() - 1]);
        // Evaluation order: every `super(...)`'s arguments (leaf first), then
        // the constructor bodies from the root. The initialization is open
        // while nothing that may run code has been evaluated since the birth.
        let mut owns = Vec::with_capacity(chain.len() + 1);
        if let Some((_, native_keys)) = native {
            owns.push(OwnKeys {
                keys: native_keys
                    .iter()
                    .map(|k| (FamilyKey::Name(k.to_string()), false))
                    .collect(),
                super_inert: false,
            });
        }
        for (depth, k) in chain.iter().enumerate().rev() {
            let own = match own_keys(
                k,
                depth + 1 < chain.len() || native.is_some(),
                fresh.contains(&k.id),
            ) {
                Ok(own) => own,
                Err(why) => {
                    refuse(c, &format!("constructor-{why}"));
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
        // A class whose chain stores no key of its own may still be filled
        // by the stores that follow its creation sites (late keys); `compute`
        // drops it when none does.
        if keys.iter().any(|k| {
            k.name()
                .is_some_and(|k| chain.iter().any(|x| accessor(x, k)))
        }) {
            refuse(c, "accessor-key");
            continue;
        }
        let parent = chain
            .get(1)
            .map(|p| p.id)
            .or(native.map(|(id, _)| id))
            .unwrap_or(0);
        if explain() {
            eprintln!(
                "perry: layout family: {} (class {}): {}",
                c.name,
                c.id,
                keys.iter()
                    .map(FamilyKey::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
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
            params: if chain.len() == 1 && !c.is_literal_shape() {
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
                // A sync generator's resume body returns raw step values; its
                // protocol results are born by the runtime on the
                // ITERATOR_RESULT layout `{value, done}`, a layout of this
                // program like any literal's.
                Expr::LinkGeneratorPrototype {
                    fixed_state: true, ..
                } => {
                    layouts.push(vec!["value".to_string(), "done".to_string()]);
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
    let candidates = colour_candidates(&positions, &reads);
    let mut out = ProgramLayouts {
        layout_count: layouts.len() as u32,
        read_keys: reads.len() as u32,
        ..Default::default()
    };
    let mut chosen: Vec<(&str, u32)> = candidates
        .iter()
        .map(|k| {
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

/// The keys that get a colour, at most [`MAX_COLOURED_KEYS`]: the most read
/// first; among keys read as often, the one more layouts hold (a colour pays
/// once per layout holding the key at its slot), then the name.
fn colour_candidates<'a>(
    positions: &HashMap<&'a str, BTreeMap<u32, u32>>,
    reads: &HashMap<String, u64>,
) -> Vec<&'a str> {
    let mut ranked: Vec<(&str, u64, u32)> = positions
        .iter()
        .map(|(k, at)| (*k, reads.get(*k).copied().unwrap_or(0), at.values().sum()))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(b.0))
    });
    ranked.truncate(MAX_COLOURED_KEYS);
    ranked.into_iter().map(|(k, _, _)| k).collect()
}

/// The slot of every key of every family layout, in key order. Slots are a
/// permutation of the layout's key positions and agree along a parent chain:
/// an inherited key takes its parent's slot. A coloured key takes its colour
/// slot when that is free and inside the layout; every other key the lowest
/// free slot.
pub fn assign_slots(
    raw: &[(u32, u32, Vec<FamilyKey>, usize)],
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
        raw: &[(u32, u32, Vec<FamilyKey>, usize)],
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
            if let Some(&(_, c)) = k.name().and_then(|n| colored.get(n)) {
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
    raw: Vec<(u32, u32, Vec<FamilyKey>, usize)>,
    mut variants: HashMap<u32, (u32, Vec<(f64, Vec<FamilyKey>)>)>,
    mut layouts: ProgramLayouts,
) -> ProgramLayouts {
    let slots = assign_slots(&raw, &layouts.colored);
    for ((class_id, parent_class_id, keys, own), slots) in raw.into_iter().zip(slots) {
        let slot_count = keys.len() as u32;
        layouts.key_total += keys.len() as u32;
        layouts.slot_total += slot_count;
        for (k, s) in keys.iter().zip(&slots) {
            if let Some(&(_, c)) = k.name().and_then(|n| layouts.colored.get(n)) {
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
        let keys: Vec<(FamilyKey, u32)> = keys.into_iter().zip(slots).collect();
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

/// Compile-time candidates only; each emitted read checks its current shape.
pub(crate) fn receiver_candidates(hir: &HirModule, layouts: &ProgramLayouts) -> BTreeMap<u32, u32> {
    late::receiver_candidates(hir, layouts)
}

/// The program's families and colouring over every module's final HIR.
pub fn compute<'a>(
    modules: impl IntoIterator<Item = (&'a str, &'a HirModule)>,
    families: bool,
    reexports: &BTreeMap<(String, String), String>,
) -> ProgramLayouts {
    let program: Vec<(&str, &HirModule)> = modules.into_iter().collect();
    let modules: Vec<&HirModule> = program.iter().map(|(_, m)| *m).collect();
    let literal_ids: BTreeSet<u32> = modules
        .iter()
        .flat_map(|m| &m.classes)
        .filter(|c| c.is_literal_shape())
        .map(|c| c.id)
        .collect();
    let colours = colouring(&modules);
    let mut refused = BTreeMap::new();
    let mut fams: Vec<RawFamily> = Vec::new();
    if families && !families_disabled() {
        fams = program_families(&program, reexports, &mut refused);
    }
    // Literal admission must not change the nominal-family use census.
    // Put them in separate compiler-only analysis slices, retaining order.
    fams.sort_by_key(|f| literal_ids.contains(&f.class_id));
    let nominal_count = fams
        .iter()
        .take_while(|f| !literal_ids.contains(&f.class_id))
        .count();
    let mut owns: Vec<usize> = fams.iter().map(|f| f.keys.len()).collect();
    let late = if fams.is_empty() || late_budget() == 0 {
        0
    } else {
        let before: usize = owns.iter().sum();
        let groups: Vec<(Option<u32>, Group)> =
            modules.iter().flat_map(|hir| module_groups(hir)).collect();
        let (nominal, literals) = fams.split_at_mut(nominal_count);
        add_variants(nominal, &groups);
        add_late_keys(nominal, &groups);
        // An unknown receiver can nominate a guarded read candidate later;
        // it cannot widen or admit a literal family merely by shared names.
        let creations: Vec<_> = groups
            .iter()
            .filter(|(this, g)| this.is_none() && g.creation && !g.classes.is_empty())
            .cloned()
            .collect();
        add_late_keys(literals, &creations);
        fams.iter().map(|f| f.keys.len()).sum::<usize>() - before
    };
    // A family that neither stores nor receives a key has the empty layout:
    // its instances are born with no slots, and a key a dynamic store adds
    // takes the object's overflow, as on any family. That layout is exact
    // unless the class's instances are records other code fills: one stored
    // into another object's property or an element (tsc's
    // `symbolLinks[id] ?? (symbolLinks[id] = new SymbolLinks())`) gets its
    // keys through loads no census here follows, so its class keeps the
    // runtime birth that learns them.
    literal::retain_late(&mut fams, &mut owns, &literal_ids);
    let filled = filled_records(&modules);
    let names: HashMap<u32, &str> = modules
        .iter()
        .flat_map(|m| &m.classes)
        .map(|c| (c.id, c.name.as_str()))
        .collect();
    let parents: BTreeSet<u32> = fams.iter().map(|f| f.parent).collect();
    let mut at = 0;
    fams.retain(|f| {
        let name = names.get(&f.class_id).copied().unwrap_or("?");
        let keep = !f.keys.is_empty() || parents.contains(&f.class_id) || !filled.contains(name);
        if !keep {
            let why = "no-keys: instances stored into a property or element";
            crate::fallback_diag::family_refused(f.class_id, why);
            *refused.entry(why.to_string()).or_default() += 1;
            if explain() {
                eprintln!("perry: layout family refused: {name}: {why}");
            }
            owns.remove(at);
        } else {
            at += 1;
        }
        keep
    });
    let variants: HashMap<u32, (u32, Vec<(f64, Vec<FamilyKey>)>)> = fams
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
    let raw: Vec<(u32, u32, Vec<FamilyKey>, usize)> = fams
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
    literal::finish(&mut layouts, &literal_ids);
    layouts.late_total = late as u32;
    layouts.refused = refused;
    // A module body is once per record; exposing the CommonJS loader/cache
    // can create another record. Refuse globally under any such capability.
    let closed = perry_hir::program_is_cjs_closed(modules.iter().copied());
    if std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2") {
        for module in &modules {
            if let Some(s) = &module.class_family_syntax {
                if s.module_capability_uses != 0 {
                    eprintln!(
                        "perry: run-once refused by module capabilities: {}: {}",
                        module.name, s.module_capability_uses
                    );
                }
            }
        }
    }
    if closed {
        for module in &modules {
            let syntax = module
                .class_family_syntax
                .as_ref()
                .expect("closed original syntax");
            let mut candidates: BTreeSet<String> = syntax.run_once_classes.clone();
            let local_definitions: BTreeSet<u32> = module
                .classes
                .iter()
                .filter(|c| !c.is_imported_stub())
                .map(|c| c.id)
                .collect();
            perry_hir::for_each_module_expr(module, &mut |e| {
                if let Expr::ClassExprFresh {
                    template,
                    evaluation_owner,
                    named_statics,
                    computed_keys,
                    computed_statics,
                    static_init_order,
                    captured_args,
                    shared_first_evaluation,
                    evaluated_parent,
                    definition_steps,
                } = e
                {
                    if evaluation_owner.is_some()
                        || !named_statics.is_empty()
                        || !computed_keys.is_empty()
                        || !computed_statics.is_empty()
                        || !static_init_order.is_empty()
                        || !captured_args.is_empty()
                        || shared_first_evaluation.is_some()
                        || evaluated_parent.is_none()
                        || !definition_steps.is_empty()
                    {
                        if candidates.remove(template)
                            && std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2")
                        {
                            eprintln!("perry: run-once retained fresh: {template}: owner={} statics={} computed={} captures={} shared={} parent={} steps={}", evaluation_owner.is_some(), named_statics.len(), computed_keys.len() + computed_statics.len(), captured_args.len(), shared_first_evaluation.is_some(), evaluated_parent.is_some(), definition_steps.len());
                        }
                    }
                }
            });
            for c in &module.classes {
                if c.extends_expr.is_some()
                    && !c.has_private_elements()
                    && c.native_extends.is_none()
                    && c.decorators.is_empty()
                    && c.computed_members.is_empty()
                    && candidates.contains(&c.name)
                    && layouts.families.get(&c.id).is_some_and(|family| {
                        family.parent_class_id != 0
                            && !local_definitions.contains(&family.parent_class_id)
                    })
                {
                    layouts.run_once_evaluated.insert(c.id);
                }
            }
        }
    }
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    layouts.digest.hash(&mut hash);
    layouts.run_once_evaluated.hash(&mut hash);
    layouts.digest = hash.finish();
    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some() {
        eprintln!("perry: canonical run-once evaluated classes: {} (closed module capabilities: {closed})", layouts.run_once_evaluated.len());
    }

    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some() {
        eprintln!("{}", layouts.report());
    }
    layouts
}

#[cfg(test)]
#[path = "layout_families/admission_tests.rs"]
mod admission_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<FamilyKey> {
        v.iter().map(|s| FamilyKey::from(*s)).collect()
    }

    #[test]
    fn a_private_element_never_equals_a_property_name() {
        let spelling = crate::codegen::static_private_class::private_storage_key(7, "#x");
        let private = FamilyKey::Private(spelling.clone());
        assert!(private != spelling.as_str());
        assert!(private != spelling);
        assert_eq!(private.name(), None);
        assert_eq!(private.spelling(), spelling);
        assert!(FamilyKey::from("x") == "x");
        assert!(FamilyKey::Name(spelling.clone()) != private);
    }

    #[test]
    fn a_fixed_state_generator_contributes_its_iterator_result_layout() {
        // The resume body returns raw step values; the runtime births the
        // protocol results on `{value, done}`. Reads of those keys through
        // receivers nothing proves keep their colour slots.
        let mut hir = HirModule::new("generator_results");
        let get = |property: &str| {
            Stmt::Expr(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: property.to_string(),
            })
        };
        hir.init = vec![
            Stmt::Expr(Expr::LinkGeneratorPrototype {
                obj: Box::new(Expr::Undefined),
                is_async: false,
                fixed_state: true,
                i32_controls: Vec::new(),
                i1_controls: Vec::new(),
            }),
            get("value"),
            get("done"),
        ];
        let colours = colouring(&[&hir]);
        assert_eq!(colours.colored("value").map(|(_, slot)| slot), Some(0));
        assert_eq!(colours.colored("done").map(|(_, slot)| slot), Some(1));
    }

    #[test]
    fn a_colour_tie_goes_to_the_key_more_layouts_hold() {
        // An enum object's members, each read once and held by one layout,
        // fill the budget; a key read as often but held by thirty layouts
        // (a base class and its subclasses) still gets its colour.
        let names: Vec<String> = (0..MAX_COLOURED_KEYS)
            .map(|i| format!("Kind{i:03}"))
            .collect();
        let mut positions: HashMap<&str, BTreeMap<u32, u32>> = HashMap::new();
        let mut reads: HashMap<String, u64> = HashMap::new();
        for (i, n) in names.iter().enumerate() {
            positions.insert(n.as_str(), BTreeMap::from([(i as u32, 1)]));
            reads.insert(n.clone(), 1);
        }
        positions.insert("spa", BTreeMap::from([(0, 30)]));
        reads.insert("spa".into(), 1);
        positions.insert("data", BTreeMap::from([(1, 1)]));
        reads.insert("data".into(), 9);
        let chosen = colour_candidates(&positions, &reads);
        assert_eq!(chosen.len(), MAX_COLOURED_KEYS);
        assert_eq!(chosen[0], "data");
        assert_eq!(chosen[1], "spa");
        assert!(!chosen.contains(&"Kind126"));
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

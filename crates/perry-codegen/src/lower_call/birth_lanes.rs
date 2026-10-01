//! Charter step 5, P4 option (a): which `number` fields of a class are born
//! on an `F64` representation lane.
//!
//! A field may be born `F64` (its slot filled with a double before any
//! constructor code runs) exactly when no JavaScript code can observe the
//! slot before the constructor writes it. The analysis walks the
//! construction of the leaf in the order the language runs it — root
//! first: each class's field initializers in declaration order, then its
//! constructor body after `super(...)` — and stops at the first event that
//! could observe the half-built instance or cut the construction short.
//!
//! Per event:
//! - a field initializer `f = e` or a top-level `this.f = e` with a
//!   `this`-free `e` is a WRITE of `f`. When `e` produces a Number it is `f`'s
//!   first write; any other first write means `f` is not a Number field at
//!   birth, so it stays `Any`;
//! - a declared field without an initializer is a define of `undefined`.
//!   For an `F64` field that define is elided (see
//!   `field_init::apply_field_initializers_recursive`); it is neither a write
//!   nor an observation here;
//! - anything else that mentions `this` (a read, a call on `this`, `this`
//!   passed out, an arrow capturing it, a `super` member access) is an
//!   OBSERVATION, and so is any statement other than a straight-line
//!   expression or `let` (control flow can make a later write conditional or
//!   skip it). The walk stops there: every field not yet written stays `Any`.
//!
//! The birth fill is `+0.0`. By construction no code reads it: the field's
//! first write precedes every observation. A later non-Number store is legal
//! and moves the object off its birth shape at runtime.
//!
//! Correctness never rests on the Number classification of a right-hand side
//! (a `number` parameter can receive a string from untyped code): every store
//! into an `F64` lane is checked where it happens. The classification only
//! keeps a class from being born on a lane its first store would leave.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Stmt, UnaryOp};

/// Per class of `chain` (root → leaf, the authoritative
/// `class_init_chains` entry), the names of the fields that class declares
/// which are born on an `F64` lane when the LEAF is constructed. Empty sets
/// everywhere when the chain cannot be analysed.
pub(crate) fn chain_birth_f64_fields(
    classes: &HashMap<String, &Class>,
    chain: &[(String, Vec<ClassField>)],
) -> Vec<(String, HashSet<String>)> {
    let empty = || {
        chain
            .iter()
            .map(|(name, _)| (name.clone(), HashSet::new()))
            .collect::<Vec<_>>()
    };
    let Some(resolved) = resolve_chain(classes, chain) else {
        return empty();
    };
    // Candidates: public, named, undecorated `number` fields, declared once on
    // the chain, with no accessor or method of the same name anywhere on it
    // (a store would dispatch to the accessor instead of writing the slot).
    let mut declared: HashMap<&str, usize> = HashMap::new();
    for class in &resolved {
        for f in &class.fields {
            *declared.entry(f.name.as_str()).or_default() += 1;
        }
    }
    let member_named = |name: &str| {
        resolved.iter().any(|c| {
            c.getters.iter().any(|(n, _)| n == name)
                || c.setters.iter().any(|(n, _)| n == name)
                || c.methods.iter().any(|m| m.name == name)
        })
    };
    let mut candidate: HashSet<String> = HashSet::new();
    for class in &resolved {
        for f in &class.fields {
            if crate::typed_shape::type_is_raw_f64_candidate(&f.ty)
                && f.key_expr.is_none()
                && !f.is_private
                && f.decorators.is_empty()
                && declared.get(f.name.as_str()) == Some(&1)
                && !member_named(&f.name)
            {
                candidate.insert(f.name.clone());
            }
        }
    }
    let mut walk = Walk {
        candidate,
        decided: HashSet::new(),
        f64: HashSet::new(),
        numbers: HashSet::new(),
    };
    'chain: for (depth, class) in resolved.iter().enumerate() {
        let ctor = class.constructor.as_ref();
        if let Some(ctor) = ctor {
            if !ctor.params.iter().all(|p| {
                p.default.is_none()
                    && !p.is_rest
                    && p.decorators.is_empty()
                    && p.arguments_object.is_none()
            }) {
                break 'chain;
            }
            walk.numbers.clear();
            for p in &ctor.params {
                if matches!(p.ty, Type::Number) {
                    walk.numbers.insert(p.id);
                }
            }
        }
        // The body after `super(...)`. A derived constructor must open with a
        // `super(...)` whose arguments cannot reach `this`; anything before it
        // could run arbitrary code ahead of the parent.
        let mut body: &[Stmt] = ctor.map(|c| c.body.as_slice()).unwrap_or(&[]);
        if depth > 0 && ctor.is_some() {
            match body.first() {
                Some(Stmt::Expr(Expr::SuperCall(args)))
                    if args.iter().all(|a| !observes_this(a)) =>
                {
                    body = &body[1..];
                }
                _ => break 'chain,
            }
        }
        for f in &class.fields {
            if f.key_expr.is_some() || f.is_private || !f.decorators.is_empty() {
                if f.init.as_ref().is_some_and(observes_this)
                    || f.key_expr.as_ref().is_some_and(observes_this)
                {
                    break 'chain;
                }
                continue;
            }
            match &f.init {
                None => {}
                Some(e) if observes_this(e) => break 'chain,
                Some(e) => walk.write(&f.name, e),
            }
        }
        for stmt in body {
            if !walk.statement(stmt) {
                break 'chain;
            }
        }
    }
    let f64 = walk.f64;
    resolved
        .iter()
        .map(|c| {
            let own = c
                .fields
                .iter()
                .filter(|f| f64.contains(&f.name))
                .map(|f| f.name.clone())
                .collect();
            (c.name.clone(), own)
        })
        .collect()
}

/// The birth rep word for `chain`: an `F64` lane at the slot of every field
/// [`chain_birth_f64_fields`] admits, `Any` everywhere else. Slots are
/// numbered as the typed layout and the class keys number them.
pub(crate) fn chain_birth_rep(
    classes: &HashMap<String, &Class>,
    chain: &[(String, Vec<ClassField>)],
) -> u64 {
    let f64: HashSet<String> = chain_birth_f64_fields(classes, chain)
        .into_iter()
        .flat_map(|(_, set)| set)
        .collect();
    let mut rep = 0u64;
    let mut slot = 0u32;
    for f in chain.iter().flat_map(|(_, fields)| fields.iter()) {
        if f.key_expr.is_some() || f.is_private {
            continue;
        }
        if f64.contains(&f.name) {
            rep |= crate::typed_shape::birth_rep_f64_lane(slot);
        }
        slot += 1;
    }
    rep
}

/// The `Class` behind each chain entry, or `None` when one is missing, is not
/// the class the chain describes (same name, other fields), or has heritage
/// this analysis cannot see through.
fn resolve_chain<'a>(
    classes: &HashMap<String, &'a Class>,
    chain: &[(String, Vec<ClassField>)],
) -> Option<Vec<&'a Class>> {
    let mut out = Vec::with_capacity(chain.len());
    for (i, (name, fields)) in chain.iter().enumerate() {
        let class = classes.get(name).copied()?;
        if class.is_imported_stub()
            || class.native_extends.is_some()
            || class.extends_expr.is_some()
            || class.heritage_lexically_shadowed
            || !class.decorators.is_empty()
            || (i == 0) != (class.extends.is_none() && class.extends_name.is_none())
            || class.fields.len() != fields.len()
            || class
                .fields
                .iter()
                .zip(fields)
                .any(|(a, b)| a.name != b.name)
        {
            return None;
        }
        out.push(class);
    }
    Some(out)
}

struct Walk {
    candidate: HashSet<String>,
    /// Fields whose first write has happened (Number or not).
    decided: HashSet<String>,
    f64: HashSet<String>,
    /// Locals known to hold a Number in the current constructor.
    numbers: HashSet<u32>,
}

impl Walk {
    fn write(&mut self, field: &str, value: &Expr) {
        if self.decided.insert(field.to_string())
            && self.candidate.contains(field)
            && self.is_number(value)
        {
            self.f64.insert(field.to_string());
        }
    }

    /// Account for one top-level constructor statement; `false` stops the
    /// walk (an observation, or control flow).
    fn statement(&mut self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Expr(Expr::PropertySet {
                object,
                property,
                value,
            }) if matches!(object.as_ref(), Expr::This) && !observes_this(value) => {
                self.write(property, value);
                true
            }
            Stmt::Expr(Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                strict: _,
            }) if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This)
                && !observes_this(value) =>
            {
                match key.as_ref() {
                    Expr::String(property) => {
                        self.write(property, value);
                        true
                    }
                    _ => false,
                }
            }
            Stmt::Expr(e) => !observes_this(e),
            Stmt::Let { id, ty, init, .. } => {
                if init.as_ref().is_some_and(observes_this) {
                    return false;
                }
                let number =
                    matches!(ty, Type::Number) || init.as_ref().is_some_and(|e| self.is_number(e));
                if number {
                    self.numbers.insert(*id);
                } else {
                    self.numbers.remove(id);
                }
                true
            }
            _ => false,
        }
    }

    fn is_number(&self, e: &Expr) -> bool {
        match e {
            Expr::Number(_) | Expr::Integer(_) => true,
            Expr::LocalGet(id) => self.numbers.contains(id),
            Expr::Unary { op, operand } => match op {
                UnaryOp::Pos => true,
                UnaryOp::Neg | UnaryOp::BitNot => self.is_number(operand),
                UnaryOp::Not => false,
            },
            // Every binary operator over two Numbers produces a Number.
            Expr::Binary { left, right, .. } => self.is_number(left) && self.is_number(right),
            _ => false,
        }
    }
}

/// Could evaluating `e` observe the instance under construction?
fn observes_this(e: &Expr) -> bool {
    match e {
        Expr::This
        | Expr::SuperCall(_)
        | Expr::SuperCallSpread(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::Closure {
            captures_this: true,
            ..
        } => true,
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(e, &mut |child: &Expr| {
                if !found && observes_this(child) {
                    found = true;
                }
            });
            found
        }
    }
}

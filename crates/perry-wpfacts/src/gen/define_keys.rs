//! Keys a definition names by a constant: `Object.defineProperties(o, map)`
//! with a literal map, or a constant binding holding one, defines each of the
//! literal's keys (`Cons::DefineAll`), and `Object.defineProperty(o, K, d)`
//! with a constant binding `K` defines key `K`, as does a class accessor
//! `get [K]()`. A lookup walks accessors only
//! for keys generation knows (`Model::accessor_atoms`), so they are named
//! before any lookup is generated.

use super::*;
use perry_hir::tdz_check::for_each_stmt_deep;
use perry_hir::{Expr, Stmt};

/// A constant binding: a local (`false`) or a module global (`true`).
type Binding = (u32, bool, u32);

/// What a constant binding is initialized with, as far as keys go.
pub(crate) enum ConstInit {
    /// An expression a constant may come of (`Gen::const_eval`, or another
    /// binding).
    Value(Expr),
    /// A literal of this class.
    Lit(String),
}

impl ConstInit {
    fn of(e: &Expr) -> Option<ConstInit> {
        match e {
            Expr::New { class_name, .. } => Some(ConstInit::Lit(class_name.clone())),
            Expr::String(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::Bool(_)
            | Expr::Undefined
            | Expr::Null
            | Expr::TypeOf(_)
            | Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Compare { .. }
            | Expr::Logical { .. }
            | Expr::Conditional { .. }
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_) => Some(ConstInit::Value(e.clone())),
            _ => None,
        }
    }
}

impl<'a> Gen<'a> {
    /// Only immutable numeric definitions may establish an allocation extent.
    pub(crate) fn owner_extent(&self, mi: u32, e: &Expr) -> Option<u32> {
        fn num(g: &Gen<'_>, mi: u32, e: &Expr, seen: &mut Vec<Binding>) -> Option<f64> {
            Some(match e {
                Expr::Integer(n) => *n as f64,
                Expr::Number(n) => *n,
                Expr::LocalGet(id) | Expr::GlobalGet(id) => {
                    let b = (mi, matches!(e, Expr::GlobalGet(_)), *id);
                    if seen.contains(&b) {
                        return None;
                    }
                    seen.push(b);
                    let fallback;
                    let v = if let Some(ConstInit::Value(v)) = g.const_inits.get(&b) {
                        v
                    } else {
                        // Declaration units share the immutable literal interface
                        // of their source module. No other unit's solved values
                        // or mutable storage are inspected here.
                        let m = g.modules[mi as usize].1;
                        fallback = if b.1 {
                            m.globals
                                .iter()
                                .find(|x| x.id == *id && !x.mutable)?
                                .init
                                .clone()?
                        } else {
                            let mut found = None;
                            for_each_stmt_deep(&m.init, &mut |s| {
                                if let Stmt::Let {
                                    id: j,
                                    mutable: false,
                                    init: Some(e),
                                    ..
                                } = s
                                {
                                    if j == id {
                                        found = Some(e.clone());
                                    }
                                }
                            });
                            found?
                        };
                        &fallback
                    };
                    let n = num(g, mi, v, seen)?;
                    seen.pop();
                    n
                }
                Expr::Binary { op, left, right } => {
                    let l = num(g, mi, left, seen)?;
                    let r = num(g, mi, right, seen)?;
                    match op {
                        perry_hir::BinaryOp::Add => l + r,
                        perry_hir::BinaryOp::Sub => l - r,
                        perry_hir::BinaryOp::Mul => l * r,
                        _ => return None,
                    }
                }
                _ => return None,
            })
        }
        let n = num(self, mi, e, &mut Vec::new())?;
        (n >= 0.0 && n <= i32::MAX as f64 && n.fract() == 0.0).then_some(n as u32)
    }

    /// The constant string `e` evaluates to: a constant expression
    /// (`const_eval`), or a constant binding whose initializer is one.
    pub(crate) fn const_key(&mut self, mi: u32, e: &Expr) -> Option<String> {
        let mut seen: Vec<Binding> = Vec::new();
        let mut e = e.clone();
        loop {
            let b = match &e {
                Expr::LocalGet(id) => (mi, false, *id),
                Expr::GlobalGet(id) => (mi, true, *id),
                // ToPropertyKey of a string is the string.
                Expr::Call { callee, args, .. }
                    if args.len() == 1
                        && matches!(callee.as_ref(),
                            Expr::ExternFuncRef { name, .. } if name == "js_to_property_key") =>
                {
                    e = args[0].clone();
                    continue;
                }
                _ => break,
            };
            if seen.contains(&b) {
                return None;
            }
            seen.push(b);
            match self.const_inits.get(&b)? {
                ConstInit::Value(x) => e = x.clone(),
                ConstInit::Lit(_) => return None,
            }
        }
        match self.const_eval(mi, &e)? {
            crate::gen_const::Const::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The keys a store under computed key `e` may name, when they are a
    /// finite set of constant strings: a constant (`const_key`), a
    /// conditional whose branches are such sets, or a constant binding
    /// initialized by one (`const k = w ? "a" : "b"`). Each is a named store.
    pub(crate) fn const_key_set(&mut self, mi: u32, e: &Expr) -> Option<Vec<String>> {
        fn go(g: &mut crate::gen::Gen, mi: u32, e: &Expr, depth: u32, out: &mut Vec<String>) -> bool {
            if depth > 8 {
                return false;
            }
            if let Some(s) = g.const_key(mi, e) {
                if !out.contains(&s) {
                    out.push(s);
                }
                return true;
            }
            match e {
                Expr::Conditional {
                    then_expr,
                    else_expr,
                    ..
                } => go(g, mi, then_expr, depth + 1, out) && go(g, mi, else_expr, depth + 1, out),
                Expr::LocalGet(id) | Expr::GlobalGet(id) => {
                    let b = (mi, matches!(e, Expr::GlobalGet(_)), *id);
                    match g.const_inits.get(&b) {
                        Some(ConstInit::Value(x)) => {
                            let x = x.clone();
                            go(g, mi, &x, depth + 1, out)
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        let mut out = Vec::new();
        go(self, mi, e, 0, &mut out).then_some(out)
    }

    /// The literal class a constant binding is born with.
    fn bound_literal(&self, b: Binding) -> Option<&String> {
        match self.const_inits.get(&b)? {
            ConstInit::Lit(class_name) => Some(class_name),
            ConstInit::Value(_) => None,
        }
    }

    /// Record every constant binding's initializer (before classes are
    /// registered: a class accessor's computed key may be one).
    pub(crate) fn note_const_inits(&mut self) {
        for mi in 0..self.modules.len() {
            let w = self.walked(mi);
            let mut inits: Vec<(Binding, ConstInit)> = Vec::new();
            {
                let mut note = |s: &Stmt| {
                    if let Stmt::Let {
                        id,
                        mutable: false,
                        init: Some(e),
                        ..
                    } = s
                    {
                        if let Some(c) = ConstInit::of(e) {
                            inits.push(((mi as u32, false, *id), c));
                        }
                    }
                };
                for s in &w.init {
                    for_each_stmt_deep(std::slice::from_ref(*s), &mut note);
                }
                for f in &w.fns {
                    for_each_stmt_deep(&f.body, &mut note);
                }
                w.each_expr(&mut |e| {
                    if let Expr::Closure { body, .. } = e {
                        for_each_stmt_deep(body, &mut note);
                    }
                });
            }
            for g in self.globals_of(mi) {
                if let (false, Some(c)) = (g.mutable, g.init.as_ref().and_then(ConstInit::of)) {
                    inits.push(((mi as u32, true, g.id), c));
                }
            }
            self.const_inits.extend(inits);
        }
    }

    /// Name the keys of the literal maps `defineProperties` is given and
    /// the keys `defineProperty` is given by a constant binding.
    pub(crate) fn note_define_keys(&mut self) {
        for mi in 0..self.modules.len() {
            let w = self.walked(mi);
            let mut lits: Vec<String> = Vec::new();
            let mut keys: Vec<Expr> = Vec::new();
            w.each_expr(&mut |e| match e {
                Expr::ObjectDefineProperties(_, props) => {
                    let name = match props.as_ref() {
                        Expr::New { class_name, .. } => Some(class_name),
                        Expr::LocalGet(id) => self.bound_literal((mi as u32, false, *id)),
                        Expr::GlobalGet(id) => self.bound_literal((mi as u32, true, *id)),
                        _ => None,
                    };
                    lits.extend(name.cloned());
                }
                Expr::ObjectDefineProperty(_, k, _)
                | Expr::ReflectDefineProperty { key: k, .. }
                    if matches!(k.as_ref(), Expr::LocalGet(_) | Expr::GlobalGet(_)) =>
                {
                    keys.push(k.as_ref().clone())
                }
                _ => {}
            });
            for k in &keys {
                if let Some(s) = self.const_key(mi as u32, k) {
                    let a = self.m.atom(&s);
                    self.m.accessor_atoms.insert(a);
                }
            }
            for name in lits {
                for n in self.literal_keys(mi, &name) {
                    let a = self.m.atom(&n);
                    self.m.accessor_atoms.insert(a);
                }
            }
        }
    }

    /// An instance accessor of a class under a computed key that is a
    /// constant string: the key it is installed under, and whether it is
    /// the setter. It is that key's accessor, as a named one is.
    pub(crate) fn named_computed(
        &mut self,
        mi: u32,
        cm: &perry_hir::ClassComputedMember,
    ) -> Option<(Atom, bool)> {
        let setter = match cm.kind {
            perry_hir::ClassComputedMemberKind::Getter => false,
            perry_hir::ClassComputedMemberKind::Setter => true,
            perry_hir::ClassComputedMemberKind::Method => return None,
        };
        if cm.is_static {
            return None;
        }
        let k = self.const_key(mi, &cm.key_expr)?;
        Some((self.m.atom(&k), setter))
    }

    /// The own keys of literal class `name` of module `mi`: its fields,
    /// accessors and methods.
    pub(crate) fn literal_keys(&self, mi: usize, name: &str) -> Vec<String> {
        let Some(c) = self.modules[mi]
            .1
            .classes
            .iter()
            .find(|c| c.name == name && c.is_literal_shape())
        else {
            return Vec::new();
        };
        c.fields
            .iter()
            .filter(|f| !f.is_private && f.key_expr.is_none())
            .map(|f| f.name.clone())
            .chain(c.getters.iter().chain(&c.setters).map(|(n, _)| n.clone()))
            .chain(c.methods.iter().map(|m| m.name.clone()))
            .collect()
    }
}

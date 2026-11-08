//! A computed key that is a parameter is shaped by its arguments. A
//! function every reference of which is the callee of a direct call in its
//! module (decision 13's closed functions: no export, never written; here
//! never passed as an argument either, `decl_units::call_only_bindings`) runs
//! only from those calls, so a parameter it never writes holds what they
//! pass: the union of the arguments' shapes (`KeyShape`), `"undefined"`
//! where a call passes none, and the shape of every value assigned to it
//! (HIR lowers a default to `if (p === undefined) p = default`; the
//! default counts as such a value either way). An
//! argument that is itself such a parameter is that parameter's union; a
//! cycle adds nothing. Any argument or assigned value without a shape, any
//! other write (`p++`), a rest parameter, a function that uses `arguments`
//! (sloppy mapped arguments alias the parameters) or a binding referenced
//! any other way (an escape, a member access, an unknown caller) gives no
//! shape. A shape over-approximates the
//! strings, so `Model::key_may_name_str` stays sound: `type[cacheKey] = v`
//! names `__proto__` only when some caller may pass it (ECMA-262 B.2.2.1.2).

use super::*;
use crate::model::KeyShape;
use perry_hir::tdz_check::{
    for_each_class_expr, for_each_expr_in, for_each_function_expr, for_each_stmts_expr,
};
use perry_hir::Expr;

/// A module's call-only functions: each parameter that holds only what the
/// calls pass, and each function's direct calls.
#[derive(Default)]
pub(crate) struct ParamCalls<'a> {
    /// Parameter local id -> (its function's binding key, its index, its
    /// default).
    params: FxHashMap<u32, ((bool, u32), usize, Option<&'a Expr>)>,
    /// Local id -> the values assigned to it (`LocalSet`).
    sets: FxHashMap<u32, Vec<&'a Expr>>,
    /// Binding key -> the argument lists of its direct calls.
    calls: FxHashMap<(bool, u32), Vec<&'a [Expr]>>,
}

impl<'a> ParamCalls<'a> {
    fn of(module: &'a perry_hir::Module) -> Self {
        let set = crate::decl_units::call_only_bindings(module);
        let mut out = ParamCalls::default();
        if set.is_empty() {
            return out;
        }
        let local_of: FxHashMap<u32, u32> = set
            .iter()
            .filter(|((func, _), _)| !*func)
            .map(|(&(_, id), &body)| (body, id))
            .collect();
        let mut written: crate::pts::FxHashSet<u32> = Default::default();
        let mut sets: FxHashMap<u32, Vec<&'a Expr>> = FxHashMap::default();
        let mut lists: Vec<((bool, u32), &'a [perry_hir::Param])> = Vec::new();
        let mut seen_body: crate::pts::FxHashSet<u32> = Default::default();
        let mut calls: FxHashMap<(bool, u32), Vec<&'a [Expr]>> = FxHashMap::default();
        {
            let f = &mut |e: &'a Expr| match e {
                Expr::Call { callee, args, .. } => {
                    let k = match callee.as_ref() {
                        Expr::FuncRef(id) => (true, *id),
                        Expr::LocalGet(id) => (false, *id),
                        _ => return,
                    };
                    if set.contains_key(&k) {
                        calls.entry(k).or_default().push(args.as_slice());
                    }
                }
                Expr::Closure {
                    func_id, params, ..
                } => {
                    if let Some(&id) = local_of.get(func_id) {
                        // One body per binding: a second literal of it is
                        // no one parameter list.
                        if !seen_body.insert(*func_id) {
                            written.extend(params.iter().map(|p| p.id));
                        }
                        lists.push(((false, id), params.as_slice()));
                    }
                }
                Expr::LocalSet(id, v) => sets.entry(*id).or_default().push(&**v),
                Expr::Update { id, .. }
                | Expr::ScopedTemp { id, .. }
                | Expr::ArrayPush { array_id: id, .. }
                | Expr::ArrayPushSpread { array_id: id, .. }
                | Expr::ArrayUnshift { array_id: id, .. }
                | Expr::ArraySplice { array_id: id, .. }
                | Expr::ArrayCopyWithin { array_id: id, .. }
                | Expr::ArrayPop(id)
                | Expr::ArrayShift(id)
                | Expr::SetAdd { set_id: id, .. } => {
                    written.insert(*id);
                }
                _ => {}
            };
            for func in &module.functions {
                for_each_function_expr(func, f);
            }
            for c in &module.classes {
                for_each_class_expr(c, f);
            }
            for g in &module.globals {
                if let Some(e) = &g.init {
                    for_each_expr_in(e, f);
                }
            }
            for_each_stmts_expr(&module.init, f);
        }
        for func in &module.functions {
            if set.contains_key(&(true, func.id)) {
                lists.push(((true, func.id), func.params.as_slice()));
            }
        }
        let mut dup: crate::pts::FxHashSet<u32> = Default::default();
        for (k, params) in lists {
            // `arguments` may alias the parameters (sloppy mapped arguments).
            if params.iter().any(|p| p.arguments_object.is_some()) {
                continue;
            }
            for (i, p) in params.iter().enumerate() {
                if p.is_rest {
                    break;
                }
                if written.contains(&p.id) {
                    continue;
                }
                if out
                    .params
                    .insert(p.id, (k, i, p.default.as_ref()))
                    .is_some()
                {
                    dup.insert(p.id);
                }
            }
        }
        for id in dup {
            out.params.remove(&id);
        }
        out.sets = sets;
        out.calls = calls;
        out
    }
}

impl<'a> Gen<'a> {
    /// The shape of computed key `e` of module `mi` (`Gen::key_shape`):
    /// its literal parts, a call of a module function's returns, or a
    /// parameter's arguments.
    pub(crate) fn key_shape_of(
        &mut self,
        mi: u32,
        e: &Expr,
        seen: &mut Vec<u32>,
    ) -> Option<KeyShape> {
        match e {
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::FuncRef(id)
                    if !self.view.is_some_and(|v| v.foreign_funcs.contains_key(id)) =>
                {
                    self.modules[mi as usize]
                        .1
                        .functions
                        .iter()
                        .find(|f| f.id == *id)
                        .and_then(KeyShape::of_returns)
                }
                _ => None,
            },
            // Per unit another unit's calls are not in the module.
            Expr::LocalGet(id) if self.view.is_none() && self.is_call_param(mi, *id) => {
                self.param_key_shape(mi, *id, seen)
            }
            // A constant binding (`Gen::const_key_set`) is its strings.
            Expr::LocalGet(_) | Expr::GlobalGet(_) => {
                self.const_key_set(mi, e).map(|ks| KeyShape {
                    globs: ks.into_iter().map(|k| vec![k]).collect(),
                })
            }
            _ => KeyShape::of(e),
        }
    }

    /// Is local `id` a parameter of one of module `mi`'s call-only
    /// functions?
    fn is_call_param(&mut self, mi: u32, id: u32) -> bool {
        if !self.param_calls.contains_key(&mi) {
            let module: &'a perry_hir::Module = self.modules[mi as usize].1;
            self.param_calls.insert(mi, ParamCalls::of(module));
        }
        self.param_calls[&mi].params.contains_key(&id)
    }

    /// The union of what the calls pass parameter `id` (module `mi`), or
    /// `None`. `seen`: the parameters being resolved (a cycle adds nothing).
    fn param_key_shape(&mut self, mi: u32, id: u32, seen: &mut Vec<u32>) -> Option<KeyShape> {
        let pc = &self.param_calls[&mi];
        let &(k, i, def) = pc.params.get(&id)?;
        if seen.contains(&id) {
            return Some(KeyShape { globs: Vec::new() });
        }
        seen.push(id);
        let calls: Vec<&'a [Expr]> = pc.calls.get(&k).cloned().unwrap_or_default();
        let sets: Vec<&'a Expr> = pc.sets.get(&id).cloned().unwrap_or_default();
        let mut globs = Vec::new();
        for args in calls {
            match args.get(i) {
                // A missing argument is `undefined` (HIR may pass it).
                None | Some(Expr::Undefined) => globs.push(vec!["undefined".to_string()]),
                Some(a) => globs.extend(self.key_shape_of(mi, a, seen)?.globs),
            }
        }
        for v in sets.into_iter().chain(def) {
            globs.extend(self.key_shape_of(mi, v, seen)?.globs);
        }
        globs.sort();
        globs.dedup();
        Some(KeyShape { globs })
    }
}

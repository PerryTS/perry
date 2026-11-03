//! The model-node constructors the exhaustive `expr_rest` match calls: the
//! primitive-protocol touches (`prim_kids`, `primbox_kids`), the container-node
//! builders (`bobj`, `new_arr_from`, `new_iter_from`), and the element/callback
//! flows (`elem_store`, `elem_load`, `callback`, `array_cb`). The match itself
//! stays in `gen_rest`: routing is one exhaustive table, not a per-group guess.
use super::*;

impl<'a> Gen<'a> {
    pub(super) fn typed_array_new(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;

        // Constructor algorithm: source iterable/array-like reads,
        // followed by numeric element conversion, not escape.
        let args: Vec<_> = kids(e).iter().map(|k| self.expr(fx, k)).collect();
        let (call, _) = self.new_call(ThisArg::Undef, args.clone(), vec![]);
        let (t, n) = self.bobj(mi, "TypedArray");
        let owner = self.begin_indexed_owner(mi, t, e);
        if let Some(&v) = args.first() {
            let vals = crate::builtins::typed_array_methods::source(&mut self.m, call, v);
            crate::builtins::string_methods::to_prim(
                &mut self.m,
                vals,
                "typed-array constructor element ToNumber/ToBigInt",
            );
            self.m.cons(v, Cons::ShareBuffer { ta: t });
            if owner {
                if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                    self.m.ta_birth_input.insert(tm, v);
                }
            }
        }
        let fa = self.m.field_any(t);
        self.m.add_prim(fa, prim::NUM | prim::BIGINT);
        n
    }

    /// The first HIR child is the receiver; the remaining children are
    /// arguments. Solve normal property lookup, including replacements
    /// and unknown receiver families, before choosing a builtin rule.
    pub(super) fn receiver_method(&mut self, fx: &Fx, e: &Expr, name: &str) -> NodeId {
        let ks = kids(e);
        let recv = self.expr(fx, ks[0]);
        let args = ks[1..].iter().map(|x| self.expr(fx, x)).collect();
        let a = self.m.atom(name);
        self.mcall(recv, Key::Named(a), args, vec![])
    }

    pub(super) fn prim_kids(
        &mut self,
        fx: &Fx,
        e: &Expr,
        result: u16,
        why: &str,
        proto: Protocol,
    ) -> NodeId {
        let ks = kids(e);
        self.prim_op(fx, &ks, result, why, proto)
    }

    /// `string.name(...operands)`: the receiver is ToString'd (the lowering
    /// pins the method to `String.prototype`), then the method runs on a
    /// string with the operands (`builtins::string_methods`).
    pub(super) fn string_method(
        &mut self,
        fx: &Fx,
        string: &Expr,
        name: &str,
        operands: &[&Box<Expr>],
    ) -> NodeId {
        let s = self.expr(fx, string);
        crate::builtins::string_methods::to_prim(&mut self.m, s, "string method receiver ToString");
        let args: Vec<NodeId> = operands.iter().map(|o| self.expr(fx, o)).collect();
        let recv = self.prim_node(prim::STR);
        let a = self.m.atom(name);
        self.mcall(recv, Key::Named(a), args, vec![])
    }

    /// A primitive container whose operands were already evaluated and are
    /// coerced by nothing.
    pub(super) fn primbox_kids_none(&mut self, fx: &Fx, why: &str) -> NodeId {
        self.prim_box_op(fx, &[], why, Protocol::OwnKeys)
    }

    pub(super) fn primbox_kids(&mut self, fx: &Fx, e: &Expr, why: &str, proto: Protocol) -> NodeId {
        let ks = kids(e);
        self.prim_box_op(fx, &ks, why, proto)
    }

    pub(crate) fn bobj(&mut self, mi: u32, what: &'static str) -> (Tok, NodeId) {
        let tm = self.new_bobj_template(mi, what);
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let n = self.tok_node(t);
        (t, n)
    }

    pub(crate) fn new_arr_from(&mut self, mi: u32, what: &str, elems: &[NodeId]) -> NodeId {
        let tm = self.new_arr_template(mi, what);
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let fa = self.m.field_any(t);
        for &e in elems {
            self.m.edge(e, fa);
        }
        self.tok_node(t)
    }

    /// An array a builtin makes from `src`'s arrays with ArraySpeciesCreate
    /// (`builtins::array_species`).
    pub(crate) fn new_species_arr(
        &mut self,
        mi: u32,
        what: &str,
        elems: &[NodeId],
        src: NodeId,
    ) -> NodeId {
        let tm = self.new_arr_template(mi, what);
        crate::builtins::array_species(&mut self.m, tm, src);
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let fa = self.m.field_any(t);
        for &e in elems {
            self.m.edge(e, fa);
        }
        self.tok_node(t)
    }

    /// A builtin iterator object (`arr.values()`, `entries()`, `keys()`)
    /// yielding `vals` through `next().value` (`builtins::iterator_method`).
    pub(crate) fn new_iter_from(&mut self, mi: u32, vals: &[NodeId]) -> NodeId {
        let (t, n) = self.bobj(mi, "Iterator");
        let a = self.m.atom("#elems");
        let e = self.m.field(t, a);
        for &v in vals {
            self.m.edge(v, e);
        }
        n
    }

    /// Store `v` into every element/entry of what `recv` holds.
    pub(super) fn elem_store(&mut self, fx: &Fx, recv: NodeId, v: NodeId, what: &str) {
        let ek = self.m.elem_key();
        self.store(fx, recv, ek, v, what.to_string(), None, false);
    }

    /// Call `cb` with `args` (this = undefined); returns the result node.
    pub(super) fn callback(&mut self, cb: NodeId, args: Vec<NodeId>) -> NodeId {
        self.call_value(cb, ThisArg::Undef, args, vec![])
    }

    /// These operations read numeric elements. The solved receiver decides
    /// whether this is an array-like; other receivers retain full lookup.
    pub(super) fn elem_load(&mut self, recv: NodeId) -> NodeId {
        let key = self.m.elem_key();
        self.load(recv, key)
    }

    pub(super) fn array_cb(
        &mut self,
        fx: &Fx,
        array: &Expr,
        callback: &Expr,
    ) -> (NodeId, NodeId, NodeId) {
        let a = self.expr(fx, array);
        let cb = self.expr(fx, callback);
        let ek = self.m.elem_key();
        let elems = self.load(a, ek);
        let num = self.prim_node(prim::NUM);
        let ret = self.callback(cb, vec![elems, num, a]);
        (a, elems, ret)
    }
}
